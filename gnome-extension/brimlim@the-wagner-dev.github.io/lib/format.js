// Human-readable times. Pure, and therefore testable: these strings are the
// only place the age of a reading is turned into something a person reads.

/**
 * "in 51 min" while the reset is close enough to feel, "Thu 14:00" once it
 * is far enough away that a countdown stops meaning anything, and "due" once
 * it has passed.
 */
export function formatReset(iso) {
    if (!iso)
        return null;
    const at = Date.parse(iso);
    if (Number.isNaN(at))
        return null;

    const seconds = Math.round((at - Date.now()) / 1000);
    if (seconds <= 0)
        return 'due';
    if (seconds >= 24 * 3600) {
        const date = new Date(at);
        // 'en-US' on purpose: the Rust port formats with chrono's English
        // weekday names, and the two must not drift apart.
        const weekday = date.toLocaleDateString('en-US', {weekday: 'short'});
        const time = `${String(date.getHours()).padStart(2, '0')}:${String(date.getMinutes()).padStart(2, '0')}`;
        return `${weekday} ${time}`;
    }

    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);

    if (hours > 0)
        return `in ${hours}h ${minutes}m`;
    if (minutes > 0)
        return `in ${minutes} min`;
    return `in ${seconds}s`;
}

export function formatAge(iso) {
    if (!iso)
        return 'never read';
    const at = Date.parse(iso);
    if (Number.isNaN(at))
        return 'never read';

    const seconds = Math.max(0, Math.round((Date.now() - at) / 1000));
    if (seconds < 60)
        return 'just now';
    const minutes = Math.floor(seconds / 60);
    if (minutes < 60)
        return `${minutes}m ago`;
    const hours = Math.floor(minutes / 60);
    if (hours < 24)
        return `${hours}h ago`;
    return `${Math.floor(hours / 24)}d ago`;
}

