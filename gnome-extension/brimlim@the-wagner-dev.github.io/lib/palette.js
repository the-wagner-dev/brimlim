// Colour is the only channel that carries "how close am I to the wall", so it
// is defined once, here, and graded the same way in every frontend.
//
// Green through yellow to red, with the green holding for the first third:
// at a fifth of a window gone nothing is wrong, and the colour should say so.

const GREEN = [0.20, 0.84, 0.29];
const YELLOW = [0.91, 0.89, 0.04];
const ORANGE = [1.00, 0.45, 0.05];
const RED = [1.00, 0.23, 0.11];

const STOPS = [
    [0.00, GREEN],
    [0.30, GREEN],
    [0.50, YELLOW],
    [0.70, ORANGE],
    [1.00, RED],
];

const TRACK = [1.00, 1.00, 1.00, 0.10];
const DISC = [0.17, 0.17, 0.18, 1.00];
/** Used wherever there is no number — never a usage colour. */
const UNKNOWN = [0.70, 0.73, 0.78, 0.55];

function lerp(a, b, t) {
    return a.map((value, i) => value + (b[i] - value) * t);
}

export function usageColor(percent) {
    const p = Math.max(0, Math.min(1, percent));
    for (let i = 1; i < STOPS.length; i += 1) {
        const [at, color] = STOPS[i];
        if (p > at)
            continue;
        const [previousAt, previousColor] = STOPS[i - 1];
        const span = at - previousAt;
        return span === 0 ? color : lerp(previousColor, color, (p - previousAt) / span);
    }
    return RED;
}

export const Colors = {
    track: TRACK,
    disc: DISC,
    unknown: UNKNOWN,
    waiting: ORANGE,
};

/** Apply an [r,g,b] or [r,g,b,a] to a Cairo context. */
export function setColor(cr, color, alpha = 1) {
    const [r, g, b, a = 1] = color;
    cr.setSourceRGBA(r, g, b, a * alpha);
}
