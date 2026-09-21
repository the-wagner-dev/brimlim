// Deciding when the notch is allowed to interrupt.
//
// Kept as a pure function of (previous state, next state) so the policy can
// be reasoned about — and tested — without a Shell around it.

const WORKING = 'working';
const WAITING = 'waiting';

function sessionsByPid(provider) {
    const map = new Map();
    for (const session of provider.sessions ?? [])
        map.set(session.pid, session);
    return map;
}

/**
 * Moments worth a reveal: a session that was working has stopped, or has
 * started waiting on the human.
 *
 * `previous` is null for the first state of a session, which deliberately
 * announces nothing — otherwise every login would chime once per session
 * that happened to be open.
 */
export function transitions(previous, next) {
    if (!previous)
        return [];

    const events = [];
    const before = new Map((previous.providers ?? []).map(p => [p.id, p]));

    for (const provider of next.providers ?? []) {
        const old = before.get(provider.id);
        if (!old)
            continue;

        const oldSessions = sessionsByPid(old);
        const newSessions = sessionsByPid(provider);

        for (const [pid, session] of newSessions) {
            const was = oldSessions.get(pid);
            if (!was)
                continue;

            if (was.state === WORKING && session.state !== WORKING) {
                events.push({
                    providerId: provider.id,
                    session: session.name,
                    kind: session.state === WAITING ? 'waiting' : 'finished',
                });
            } else if (was.state !== WAITING && session.state === WAITING) {
                events.push({providerId: provider.id, session: session.name, kind: 'waiting'});
            }
        }

        // A session that vanished while working finished its work by leaving.
        for (const [pid, was] of oldSessions) {
            if (was.state === WORKING && !newSessions.has(pid))
                events.push({providerId: provider.id, session: was.name, kind: 'finished'});
        }
    }

    return events;
}
