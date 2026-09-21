#!/usr/bin/env bash
# Loads the extension into a throwaway GNOME Shell on a private bus, and
# reports whether it enabled, survived the daemon dying, and tore down clean.
#
#   tools/nested-shell.sh
#
# The isolation gotcha, learned the hard way: exporting XDG_CONFIG_HOME
# *inside* a dbus-run-session script does NOT isolate gsettings. dconf is a
# bus-activated service, and an activated service inherits the *bus daemon's*
# environment, not the caller's — so the writes land in the real
# ~/.config/dconf/user and disable the extensions of the live session. The
# variable has to be exported before dbus-run-session starts, which is what
# this script does.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UUID="brimlim@the-wagner-dev.github.io"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

DAEMON="${DAEMON:-$ROOT/target/debug/brimlimd}"
[ -x "$DAEMON" ] || { echo "build the daemon first: cargo build"; exit 1; }

export XDG_CONFIG_HOME="$WORK/config"
export XDG_STATE_HOME="$WORK/state"
# The working tree, not whatever happens to be installed: a harness that
# tests a stale copy is worse than no harness.
export XDG_DATA_HOME="$WORK/data"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_DATA_HOME/gnome-shell/extensions"
cp -r "$ROOT/gnome-extension/$UUID" "$XDG_DATA_HOME/gnome-shell/extensions/$UUID"
SCHEMAS="$XDG_DATA_HOME/gnome-shell/extensions/$UUID/schemas"
glib-compile-schemas "$SCHEMAS"

cat > "$WORK/session.sh" <<EOF
#!/bin/bash
set -u
gsettings set org.gnome.shell disable-user-extensions false
gsettings set org.gnome.shell enabled-extensions "[]"

"$DAEMON" > "$WORK/daemon.log" 2>&1 &
DAEMON_PID=\$!

BRIMLIM_DEBUG=1 gnome-shell --headless --virtual-monitor 1280x800 --wayland --no-x11 \
    > "$WORK/shell.log" 2>&1 &
SHELL_PID=\$!
sleep 12

info() {
    gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \\
        --method org.gnome.Shell.Extensions.GetExtensionInfo '$UUID' 2>&1 \\
        | tr ',' '\\n' | grep -iE "'state'|'error'" | tr -d " '"
}

gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \\
    --method org.gnome.Shell.Extensions.EnableExtension '$UUID' > /dev/null
sleep 5
echo "enabled:          \$(info)"

# The notch starts collapsed, which now means a pill with no size at all.
# Everything after this runs with it out, so the allocation the Shell reports
# at the end is the revealed one.
gsettings --schemadir "$SCHEMAS" set org.gnome.shell.extensions.brimlim mode always-visible
sleep 4
echo "always-visible:   \$(info)"

kill \$DAEMON_PID; sleep 4
echo "daemon killed:    \$(info)"

"$DAEMON" >> "$WORK/daemon.log" 2>&1 &
DAEMON_PID=\$!
sleep 6
echo "daemon restarted: \$(info)"

gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \\
    --method org.gnome.Shell.Extensions.DisableExtension '$UUID' > /dev/null
sleep 3
echo "disabled:         \$(info)"

kill \$SHELL_PID \$DAEMON_PID 2>/dev/null || true
wait \$SHELL_PID 2>/dev/null || true
EOF
chmod +x "$WORK/session.sh"

timeout 90 dbus-run-session -- "$WORK/session.sh" 2>"$WORK/bus.log" \
    | grep -E '^(enabled|always|daemon|disabled)'

# "It enabled without errors" is not the same as "it drew something": an actor
# that never gets an allocation fails silently and looks perfectly healthy.
# `|| true`: under `set -e` a grep that finds nothing would kill the script
# before it could say what was missing.
GEOMETRY="$(grep -o 'BRIMLIM-GEOMETRY.*' "$WORK/shell.log" | tail -1 || true)"
if [ -z "$GEOMETRY" ]; then
    echo "geometry:         NONE — the pill was never allocated"
    exit 1
fi
echo "geometry:         ${GEOMETRY#BRIMLIM-GEOMETRY }"
# The last line is the revealed pill, because the run switches to
# always-visible and stays there.
case "$GEOMETRY" in
    *pill=0x*|*x0\ *) echo "geometry:         FAILED — the revealed pill has no size"; exit 1 ;;
    *shown=false*) echo "geometry:         FAILED — the revealed pill is not mapped"; exit 1 ;;
esac

# And the collapsed notch really is nothing, rather than a full-size actor
# drawing a small shape — which would keep owning input it does not cover.
if grep -q 'BRIMLIM-GEOMETRY .*shown=false' "$WORK/shell.log"; then
    echo "collapsed:        the pill unmaps and gives its pixels back"
else
    echo "collapsed:        FAILED — the pill never unmapped while collapsed"
    exit 1
fi

echo "complaints in the shell log: $(grep -icE 'brimlim.*(error|Stack)' "$WORK/shell.log" || true)"
