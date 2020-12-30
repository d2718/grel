# `grel`

an IRC-like chat server/client in Rust

### Overview

`grel` is a simple IRC-like chat server with a simple, easily-extensible
JSON-based protocol. The `greld` server is meant to be small and easy to
configure. There is a "reference" client, `grel`, also written in Rust,
but the protocol is meant to be sufficiently simple that clients (with a
variety of features) should be easily implementable in any number of languages.

### State

Both `greld` and `grel` work, at least on Debian 10, and, I suspect, any
vaguely POSIX-y system that sports Rust's `cargo`. There are still features
as yet to be implemented, like blocking, private messaging, and channel
(or `Room`, in the parlance of this particular piece of software) operator
functions.

TODO:

  * Spawning a new thread solely to catch `SIGWINCH` seems overkill. I don't
    know what I was thinking. I'm just going to have the client check the
    terminal size every time through its loop to detect changes.

  * Rate-limiting. There is some provision for rate-limiting built into
    the `User` struct, but currently the server does nothing with it.

  * Users should be able to send private messages to each other.

  * Users should be able to "block" specific other users and not see their
    messages

  * Room ("channel" in the classic IRC sense) operators should be able to
    exercise certain regulatory influence over their Rooms, like muting
    or ejecting specific users.

  * Eventually, I would like things like blocks/mutes/bans to be IP-specific,
    but that will require saving more state, and interacting more heavily
    with socket addresses.

I am happy to entertain feature requests, but simplicity is a goal.

### More...
