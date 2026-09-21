#!/usr/bin/gjs -m
// Unit tests for the parts of the extension that are pure functions.
//
//   gjs -m tools/test.js
//
// Everything imported here is Shell-free by construction; the widgets that
// need a stage are exercised by loading the extension in a nested shell.

import {transitions} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/announce.js';
import {Edge} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/edges.js';
import {formatAge, formatReset} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/format.js';
import {Metrics, pillSize, placement, setScale, tongueBox}
    from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/geometry.js';
import {usageColor} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/palette.js';

let failures = 0;
let count = 0;

function check(name, fn) {
    count += 1;
    try {
        fn();
        print(`  ok   ${name}`);
    } catch (error) {
        failures += 1;
        print(`  FAIL ${name}\n       ${error.message}`);
    }
}

function assert(condition, message) {
    if (!condition)
        throw new Error(message ?? 'assertion failed');
}

function equal(actual, expected, message) {
    const a = JSON.stringify(actual);
    const b = JSON.stringify(expected);
    if (a !== b)
        throw new Error(`${message ?? 'not equal'}: got ${a}, want ${b}`);
}

const session = (pid, name, state) => ({pid, name, state});
const state = sessions => ({
    providers: [{id: 'claude', sessions}],
});

print('announce');
check('the first reading after startup announces nothing', () => {
    equal(transitions(null, state([session(1, 'repo', 'working')])), []);
});
check('a session that stops working is a finish', () => {
    const events = transitions(
        state([session(1, 'repo', 'working')]),
        state([session(1, 'repo', 'idle')]));
    equal(events, [{providerId: 'claude', session: 'repo', kind: 'finished'}]);
});
check('a session that starts waiting is announced once, not twice', () => {
    const events = transitions(
        state([session(1, 'repo', 'working')]),
        state([session(1, 'repo', 'waiting')]));
    equal(events.length, 1);
    equal(events[0].kind, 'waiting');
});
check('a session that vanishes mid-turn counts as finished', () => {
    const events = transitions(state([session(1, 'repo', 'working')]), state([]));
    equal(events, [{providerId: 'claude', session: 'repo', kind: 'finished'}]);
});
check('a quiet tick announces nothing', () => {
    const now = state([session(1, 'repo', 'idle')]);
    equal(transitions(now, now), []);
});
check('a session appearing already busy is not an event', () => {
    equal(transitions(state([]), state([session(1, 'repo', 'working')])), []);
});
check('a provider seen for the first time is not replayed', () => {
    const before = {providers: []};
    const after = {providers: [{id: 'codex', sessions: [session(9, 'x', 'waiting')]}]};
    equal(transitions(before, after), []);
});

print('geometry — the HiDPI contract');
check('pill size at scale 2 is exactly twice the scale 1 size', () => {
    setScale(1);
    const one = pillSize(Edge.RIGHT, 2);
    setScale(2);
    const two = pillSize(Edge.RIGHT, 2);
    equal(two.map(v => v / 2), one, 'logical size must not drift with scale');
});
check('the tongue keeps its logical 4x80 at any scale', () => {
    setScale(1);
    const [, , w1, h1] = tongueBox(Edge.RIGHT, pillSize(Edge.RIGHT, 2));
    equal([w1, h1], [Metrics.tongueThickness, Metrics.tongueLength]);
    setScale(2);
    const [, , w2, h2] = tongueBox(Edge.RIGHT, pillSize(Edge.RIGHT, 2));
    equal([w2 / 2, h2 / 2], [Metrics.tongueThickness, Metrics.tongueLength]);
});
check('placement is identical in logical pixels across scales', () => {
    setScale(1);
    const size1 = pillSize(Edge.RIGHT, 2);
    const spot1 = placement(Edge.RIGHT, {x: 0, y: 0, width: 1280, height: 800}, size1);
    setScale(2);
    const size2 = pillSize(Edge.RIGHT, 2);
    const spot2 = placement(Edge.RIGHT, {x: 0, y: 0, width: 2560, height: 1600}, size2);

    equal(spot2.x / 2, spot1.x);
    equal(spot2.y / 2, spot1.y);
    equal(spot2.hiddenOffset.map(v => v / 2), spot1.hiddenOffset);
});
check('every edge anchors flush against its own side', () => {
    setScale(1);
    const monitor = {x: 100, y: 50, width: 1280, height: 800};
    for (const edge of [Edge.TOP, Edge.RIGHT, Edge.BOTTOM, Edge.LEFT]) {
        const size = pillSize(edge, 2);
        const [w, h] = size;
        const spot = placement(edge, monitor, size);
        switch (edge) {
        case Edge.RIGHT: equal(spot.x + w, monitor.x + monitor.width, 'right'); break;
        case Edge.LEFT: equal(spot.x, monitor.x, 'left'); break;
        case Edge.TOP: equal(spot.y, monitor.y, 'top'); break;
        case Edge.BOTTOM: equal(spot.y + h, monitor.y + monitor.height, 'bottom'); break;
        }
    }
});
check('the hidden offset always moves the pill out through its own edge', () => {
    setScale(1);
    const size = pillSize(Edge.LEFT, 1);
    equal(placement(Edge.LEFT, {x: 0, y: 0, width: 800, height: 600}, size).hiddenOffset,
        [-size[0], 0]);
    const top = pillSize(Edge.TOP, 1);
    equal(placement(Edge.TOP, {x: 0, y: 0, width: 800, height: 600}, top).hiddenOffset,
        [0, -top[1]]);
});

print('format');
check('a future reset reads as a countdown', () => {
    const iso = new Date(Date.now() + 3 * 3600 * 1000 + 25 * 60 * 1000).toISOString();
    equal(formatReset(iso), 'in 3h 25m');
    equal(formatReset(new Date(Date.now() + 51 * 60 * 1000).toISOString()), 'in 51 min');
});

check('a reset more than a day out is a date, not a countdown', () => {
    const text = formatReset(new Date(Date.now() + 30 * 3600 * 1000).toISOString());
    assert(!text.startsWith('in '), `a day-away countdown means nothing: ${text}`);
    assert(text.includes(':'), `it should name a time of day: ${text}`);
});
check('a reset in the past reads as due, never as a negative time', () => {
    equal(formatReset(new Date(Date.now() - 1000).toISOString()), 'due');
});
check('a missing reset time produces nothing rather than a guess', () => {
    equal(formatReset(null), null);
    equal(formatReset('not a date'), null);
});
check('an unread provider says so instead of showing an age', () => {
    equal(formatAge(null), 'never read');
});
check('a fresh reading reads as just now', () => {
    equal(formatAge(new Date().toISOString()), 'just now');
});

print('palette');
check('the grade runs accent to red and never wraps round', () => {
    const [r0, , b0] = usageColor(0);
    const [r1, , b1] = usageColor(1);
    assert(b0 > r0, 'an empty window should read as accent blue');
    assert(r1 > b1, 'a full window should read as red');
});
check('redness only ever increases with usage', () => {
    let previous = -1;
    for (let p = 0; p <= 1.0001; p += 0.05) {
        const [r] = usageColor(p);
        assert(r >= previous - 1e-9, `red went backwards at ${p.toFixed(2)}`);
        previous = r;
    }
});
check('out-of-range input is clamped, not extrapolated', () => {
    equal(usageColor(-5), usageColor(0));
    equal(usageColor(17), usageColor(1));
});

print('');
print(`${count - failures}/${count} passed`);
if (failures > 0)
    imports.system.exit(1);
