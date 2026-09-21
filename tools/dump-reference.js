#!/usr/bin/gjs -m
// Dumps the extension's geometry and palette as a reference table, so the
// Rust transliteration can be checked against the original rather than
// against someone's memory of it.
//
//   gjs -m tools/dump-reference.js > crates/brimlim-gtk/fixtures/reference.json

import {Edge} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/edges.js';
import {pillSize, ringOrigin, setScale, tongueBox}
    from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/geometry.js';
import {usageColor} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/palette.js';

setScale(1);

const palette = [];
for (let step = 0; step <= 100; step += 1) {
    const percent = step / 100;
    palette.push([percent, ...usageColor(percent)]);
}

const geometry = [];
for (const edge of [Edge.TOP, Edge.RIGHT, Edge.BOTTOM, Edge.LEFT]) {
    for (let count = 1; count <= 4; count += 1) {
        const size = pillSize(edge, count);
        geometry.push({
            edge,
            count,
            pill: size,
            tongue: tongueBox(edge, size),
            rings: Array.from({length: count}, (_, i) => ringOrigin(edge, i)),
        });
    }
}

print(JSON.stringify({palette, geometry}, null, 1));
