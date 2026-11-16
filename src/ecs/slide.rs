//! Drives the snapshot slide of the "rows" workspace animation (see
//! [`crate::slide`]).
//!
//! 1. [`begin_workspace_slide`] runs just before `show_active_workspace`, while
//!    the real windows still show the old workspace: it captures the display
//!    and covers it with that image.
//! 2. `show_active_workspace` switches the strips instantly underneath.
//! 3. [`advance_workspace_slide`] runs once that switch has been committed to
//!    the OS and slides the cover off the display, up for a higher-numbered
//!    workspace and down for a lower one, uncovering the new windows live.
//!    Once the slide has run its course it takes the cover down.
//!
//! A slide lasts at most its duration plus a short grace period, so the display
//! is never left covered.

use std::time::{Duration, Instant};

use bevy::app::{App, Plugin, PostUpdate, Update};
use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::hierarchy::ChildOf;
use bevy::ecs::query::{Added, Has, With};
use bevy::ecs::schedule::IntoScheduleConfigs as _;
use bevy::ecs::schedule::common_conditions::resource_changed;
use bevy::ecs::system::{Commands, Local, NonSendMut, Populated, Query, Res, Single};
use bevy::math::IRect;
use tracing::{debug, instrument};

use super::layout::LayoutStrip;
use super::workspace::{PreviousStripPosition, show_active_workspace};
use super::{ActiveWorkspaceMarker, DockPosition, PostUpdateSet, UpdateSet};
use crate::config::Config;
use crate::manager::Display;
use crate::slide::{self, WorkspaceSlideOverlay};

/// Grace period after the slide's own duration before the cover comes down.
const SLIDE_GRACE: Duration = Duration::from_millis(30);
/// An `animation_speed` at or above this means animations are off.
const ANIMATIONS_OFF_SPEED: f64 = 1000.0;

/// A workspace switch being animated on one display.
#[derive(Component, Debug)]
pub(crate) struct WorkspaceSlide {
    from_below: bool,
    duration: Duration,
    phase: SlidePhase,
}

#[derive(Debug)]
enum SlidePhase {
    /// Covered, while the windows underneath switch.
    Covered,
    /// The cover is moving off the display; it is taken down at `until`.
    Sliding { until: Instant },
}

pub(crate) struct WorkspaceSlidePlugin;

impl Plugin for WorkspaceSlidePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            begin_workspace_slide
                .run_if(|config: Res<Config>| config.workspace_slide())
                .before(show_active_workspace)
                .in_set(UpdateSet::Workspace),
        );
        app.add_systems(
            Update,
            warm_up_screen_capture.run_if(resource_changed::<Config>),
        );
        app.add_systems(
            PostUpdate,
            advance_workspace_slide.after(PostUpdateSet::Animate),
        );
    }
}

/// Whether the new workspace comes in from below: higher-numbered rows sit
/// below lower ones. `None` when there is nothing to slide between.
fn slide_from_below(source: u32, target: u32) -> Option<bool> {
    (source != target).then_some(target > source)
}

/// The slide's length for an `animation_speed` (an exponential decay rate),
/// matching how long a window animation takes to look settled at that speed.
/// `None` when animations are off.
fn slide_duration(speed: f64) -> Option<Duration> {
    (speed > 0.0 && speed < ANIMATIONS_OFF_SPEED)
        .then(|| Duration::from_secs_f64((5.0 / speed).clamp(0.15, 0.6)))
}

/// The part of a display the slide covers: its full width, between the edge
/// paddings vertically, so a bar in the top padding stays put.
fn slide_rect(display: IRect, viewport: IRect) -> IRect {
    IRect::new(display.min.x, viewport.min.y, display.max.x, viewport.max.y)
}

type SlideStrips<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static LayoutStrip,
        &'static ChildOf,
        Has<PreviousStripPosition>,
    ),
>;

#[allow(
    clippy::too_many_arguments,
    reason = "a Bevy system: each parameter is a separate world access"
)]
#[instrument(level = "debug", skip_all)]
fn begin_workspace_slide(
    activated: Single<Entity, Added<ActiveWorkspaceMarker>>,
    strips: SlideStrips,
    displays: Query<(&Display, Option<&DockPosition>)>,
    slides: Query<Entity, With<WorkspaceSlide>>,
    overlay: Option<NonSendMut<WorkspaceSlideOverlay>>,
    config: Res<Config>,
    mut asked_permission: Local<bool>,
    mut commands: Commands,
) {
    let Some(mut overlay) = overlay else {
        return;
    };
    let Ok((_, target, child_of, _)) = strips.get(*activated) else {
        debug!("activated workspace has no strip; not sliding");
        return;
    };
    // The row on screen until now: same native workspace and display, and not
    // parked (a parked strip remembers where it was shown).
    let Some(source) = strips
        .iter()
        .find(|(entity, strip, parent, parked)| {
            *entity != *activated
                && !parked
                && strip.id() == target.id()
                && parent.parent() == child_of.parent()
        })
        .map(|(_, strip, _, _)| strip)
    else {
        debug!("no workspace was shown on this display before; not sliding");
        return;
    };
    let Some(from_below) = slide_from_below(source.virtual_index, target.virtual_index) else {
        debug!(index = target.virtual_index, "same workspace; not sliding");
        return;
    };
    let Some(duration) = slide_duration(config.animation_speed()) else {
        debug!("animations are off; not sliding");
        return;
    };
    if !slide::screen_capture_allowed() {
        debug!("no Screen Recording permission; switching instantly");
        if !*asked_permission {
            *asked_permission = true;
            slide::request_screen_capture();
        }
        return;
    }
    let Ok((display, dock)) = displays.get(child_of.parent()) else {
        return;
    };

    // A slide still running is superseded: capture what is really on screen
    // underneath it (its windows already sit at its destination) and reuse
    // the cover.
    for entity in &slides {
        commands.entity(entity).despawn();
    }
    let rect = slide_rect(
        display.bounds(),
        display.actual_display_bounds(dock, &config),
    );
    let Some(image) = slide::capture(rect, overlay.window_id()) else {
        debug!("capturing the outgoing workspace failed; switching instantly");
        overlay.hide();
        return;
    };
    debug!(
        source = source.virtual_index,
        target = target.virtual_index,
        from_below,
        "covering the display for a workspace slide"
    );
    overlay.cover(rect, &image);
    commands.spawn(WorkspaceSlide {
        from_below,
        duration,
        phase: SlidePhase::Covered,
    });
}

#[instrument(level = "debug", skip_all)]
fn advance_workspace_slide(
    mut slides: Populated<(Entity, &mut WorkspaceSlide)>,
    overlay: Option<NonSendMut<WorkspaceSlideOverlay>>,
    mut commands: Commands,
) {
    let Some(mut overlay) = overlay else {
        // Nothing is covered without an overlay (as in tests).
        for (entity, _) in &slides {
            commands.entity(entity).despawn();
        }
        return;
    };
    let now = Instant::now();
    for (entity, mut slide) in &mut slides {
        match slide.phase {
            // This runs after the switch has been committed to the OS this
            // frame, so the windows the cover uncovers are already the new ones.
            SlidePhase::Covered => {
                debug!(upwards = slide.from_below, "sliding the cover away");
                overlay.slide_away(slide.from_below, slide.duration.as_secs_f64());
                slide.phase = SlidePhase::Sliding {
                    until: now + slide.duration + SLIDE_GRACE,
                };
            }
            SlidePhase::Sliding { until } => {
                if now >= until {
                    overlay.hide();
                    commands.entity(entity).despawn();
                }
            }
        }
    }
}

/// The first capture in a process is several times slower than the rest, so
/// take a throwaway one as soon as the slide is configured.
fn warm_up_screen_capture(config: Res<Config>, mut warmed: Local<bool>) {
    if *warmed || !config.workspace_slide() || !slide::screen_capture_allowed() {
        return;
    }
    *warmed = true;
    let _ = slide::capture(IRect::new(0, 0, 1, 1), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn higher_numbered_rows_come_from_below() {
        assert_eq!(slide_from_below(0, 1), Some(true));
        assert_eq!(slide_from_below(3, 1), Some(false));
        assert_eq!(slide_from_below(2, 2), None);
    }

    #[test]
    fn slide_duration_follows_animation_speed() {
        assert_eq!(slide_duration(1_000_000.0), None, "animations off");
        assert_eq!(slide_duration(0.0), None);
        let fast = slide_duration(20.0).unwrap();
        let slow = slide_duration(10.0).unwrap();
        assert!(fast < slow);
        assert_eq!(slide_duration(1.0), Some(Duration::from_millis(600)));
        assert_eq!(slide_duration(500.0), Some(Duration::from_millis(150)));
    }

    #[test]
    fn slide_rect_spans_the_display_between_paddings() {
        let display = IRect::new(0, 25, 2048, 1152);
        let viewport = IRect::new(10, 60, 2038, 1142);
        assert_eq!(slide_rect(display, viewport), IRect::new(0, 60, 2048, 1142));
    }
}
