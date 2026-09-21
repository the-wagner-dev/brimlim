#!/usr/bin/gjs -m
// Dumps the extension's geometry and palette as a reference table, so the
// Rust transliteration can be checked against the original rather than
// against someone's memory of it.
//
//   gjs -m tools/dump-reference.js > crates/brimlim-gtk/fixtures/reference.json

import {Edge} from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/edges.js';
import {pillSize, revealShape, ringOrigin, setScale, tongueBox}
    from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/geometry.js';
import {Colors, usageColor}
    from '../gnome-extension/brimlim@the-wagner-dev.github.io/lib/palette.js';

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

// The reveal is one pure function of progress, and it is the only piece of
// the drawing that both ports animate frame by frame — so a drift of a pixel
// here is a drift that moves, which is far more visible than a static one.
const reveal = [];
for (const edge of [Edge.TOP, Edge.RIGHT, Edge.BOTTOM, Edge.LEFT]) {
    for (const count of [1, 3]) {
        const size = pillSize(edge, count);
        for (let step = 0; step <= 20; step += 1) {
            const t = step / 20;
            const shape = revealShape(edge, size, t);
            reveal.push({
                edge,
                count,
                t,
                box: [shape.x, shape.y, shape.width, shape.height],
                radius: shape.radius,
                flare: shape.flare,
                cells: shape.cells,
                tongue: shape.tongue,
            });
        }
    }
}

// The colours that are deliberately *not* on the grade travel with it: they
// mean something other than "how close to the wall", and the two ports have
// to be wrong together or not at all.
const colors = {
    track: Colors.track,
    disc: Colors.disc,
    unknown: Colors.unknown,
    waiting: Colors.waiting,
};

print(JSON.stringify({palette, colors, geometry, reveal}, null, 1));
