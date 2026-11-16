#!/usr/bin/env bash
# Build this checkout, install it as the running Paneru, and restart the service.
#
# The binary is signed with a stable self-signed identity so macOS keeps the
# Accessibility / Input Monitoring grants across rebuilds (ad-hoc signatures
# change every build, and TCC would ask again each time). Create the identity
# once with `scripts/dogfood.sh --create-identity`.
#
#   scripts/dogfood.sh                  build, sign, install, restart
#   scripts/dogfood.sh --no-build       reinstall the last release build
#   scripts/dogfood.sh --create-identity
set -euo pipefail

IDENTITY_NAME="${PANERU_SIGN_IDENTITY:-Paneru Local Signing}"
BUNDLE_ID="${PANERU_BUNDLE_ID:-com.github.anguswww.paneru}"
INSTALL_PATH="${PANERU_INSTALL_PATH:-$HOME/.local/bin/paneru}"
LABEL="com.github.karinushka.paneru"
PLIST="$HOME/Library/LaunchAgents/$LABEL.plist"
REPO="$(cd "$(dirname "$0")/.." && pwd)"

create_identity() {
    if security find-identity -p codesigning | grep -q "\"$IDENTITY_NAME\""; then
        echo "identity \"$IDENTITY_NAME\" already exists"
        return
    fi
    local tmp pw
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' RETURN
    cat >"$tmp/cs.cnf" <<EOF
[req]
distinguished_name=dn
x509_extensions=ext
prompt=no
[dn]
CN=$IDENTITY_NAME
[ext]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
EOF
    openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
        -keyout "$tmp/key.pem" -out "$tmp/cert.pem" -config "$tmp/cs.cnf" 2>/dev/null
    pw="$(openssl rand -hex 12)"
    openssl pkcs12 -export -legacy -name "$IDENTITY_NAME" \
        -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -out "$tmp/id.p12" -passout "pass:$pw"
    security import "$tmp/id.p12" -k "$HOME/Library/Keychains/login.keychain-db" \
        -P "$pw" -T /usr/bin/codesign
    echo "created identity \"$IDENTITY_NAME\""
}

identity_hash() {
    security find-identity -p codesigning |
        awk -v name="\"$IDENTITY_NAME\"" 'index($0, name) { print $2; exit }'
}

case "${1:-}" in
--create-identity)
    create_identity
    exit 0
    ;;
--no-build) ;;
"") (cd "$REPO" && cargo build --release) ;;
*)
    echo "usage: $0 [--no-build | --create-identity]" >&2
    exit 2
    ;;
esac

hash="$(identity_hash)"
if [[ -z "$hash" ]]; then
    echo "no \"$IDENTITY_NAME\" signing identity; run: $0 --create-identity" >&2
    exit 1
fi

mkdir -p "$(dirname "$INSTALL_PATH")"
staged="$INSTALL_PATH.new"
cp "$REPO/target/release/paneru" "$staged"
# No hardened runtime: LuaJIT needs to map executable memory.
codesign --force --sign "$hash" --identifier "$BUNDLE_ID" "$staged"
mv -f "$staged" "$INSTALL_PATH"
echo "installed $("$INSTALL_PATH" --version) ($(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)) to $INSTALL_PATH"

if [[ ! -f "$PLIST" ]]; then
    "$INSTALL_PATH" service install
fi
if [[ "$(/usr/libexec/PlistBuddy -c 'Print :Program' "$PLIST")" != "$INSTALL_PATH" ]]; then
    /usr/libexec/PlistBuddy -c "Set :Program $INSTALL_PATH" "$PLIST"
    launchctl bootout "gui/$(id -u)/$LABEL" 2>/dev/null || true
    # bootout returns before the old instance has finished its exit cleanup;
    # bootstrapping meanwhile fails with "5: Input/output error".
    for attempt in 1 2 3 4 5 6 7 8 9 10; do
        launchctl bootstrap "gui/$(id -u)" "$PLIST" 2>/dev/null && break
        [[ $attempt == 10 ]] && launchctl bootstrap "gui/$(id -u)" "$PLIST"
        sleep 1
    done
    echo "pointed $LABEL at $INSTALL_PATH"
else
    launchctl kickstart -k "gui/$(id -u)/$LABEL"
fi
echo "restarted $LABEL"
