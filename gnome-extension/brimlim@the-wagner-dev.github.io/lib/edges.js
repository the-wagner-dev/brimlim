// Edge identity, kept free of any Shell import so the pure drawing modules
// can be rendered outside a running Shell.

export const Edge = {
    TOP: 'top',
    RIGHT: 'right',
    BOTTOM: 'bottom',
    LEFT: 'left',
};

export function isVertical(edge) {
    return edge === Edge.LEFT || edge === Edge.RIGHT;
}
