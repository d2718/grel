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
functions. (See the TODO sections, below, for more details.)

### Installation

Clone the repo and everything should `cargo build --release` just fine. This
will build both the `grel` client and the `greld` server.

### Client instructions

#### Configuration

Run `grel -g` to generate a default config file; this will show up at
`your_normal_config_dir/grel/grel.toml`. The defaults are sane, but you'll
probably want to at least set the server address. The config options (and
their default values) are

  * `address = '127.0.0.1:51516'`
     The IP address and port of the server to which you'd like to connect.
     This can be overridden with the `-a` command-line option.

  * `name = 'grel user'`
    The name you'd like to (attempt to) use when connecting to the server.
    If a sufficiently similar name is taken, you will be renamed to something
    generic (but unique). This can be overridden with the `-n` command-line
    option.
    
  * `timeout_ms = 100`
    Minimum amount of time (in milliseconds) per iteration of the main loop.
    Setting this to a smaller value will make typing feel snappier, but may
    consume more system resources.

  * `block_ms = 5000`
    I don't actually think this does anything right now.

  * `read_size = 1024`
    The the amount (in bytes) the client attempts to read from its connection
    with the server each time through the main loop. The default amount
    is almost undoubtedly fine. Setting this to 0 will render the client
    inoperable; setting this to a very low number will impact your experience.

  * `roster_width = 24`
    Number of characters wide to draw the panel that holds the current `Room`'s
    roster. By default, the server limits user names to 24 characters, so this
    is a reasonable width.

  * `cmd_char = ';'`
    The character prepended to an input line to indicate to the client it
    should be interpreted as a _command_, rather than just as text to be
    sent to the chat. These instructions will assume the default value.

  * `max_scrollback = 2000`
    The maximum number of lines to keep in the scrollback buffer.

  * `min_scrollback = 1000`
    When the scrollback buffer reaches `max_scrollback`, it will be trimmed
    to this many lines. For reasons that should be obvious, this must be
    smaller than `max_scrollback`.

#### Use

The client's operation is _modal_ (_a la_ `vi`). Because I am more
sympathetic than Bill Joy, the client launches in _input_ mode, where text you
type gets added to the input line and sent to the server when you hit return.
This is indicated by the `Ipt` in the lower-left-hand corner. In this made,
the backspace, delete, and horizontal arrow keys act as you'd expect.

Hitting escape (or backspace when the input line is empty) will put you in
_command_ mode (indicated by `Com` in the lower-left-hand corner), where you
will eventually be able to control more aspects of the client. Right now,

  * `q` will quit with no leave message.
  
  * `PgUp/PgDn` will scroll the chat text up/down one screen.
  
  * The up/down arrow keys will scroll the chat text up/down one line.

You can also type some server-interaction commands from input mode. For
example,

  * `;quit Y'all're losers!1` will disconnect from the server, showing the
    message, "Y'all're losers!1" to everyone in the Room.

  * `;name xXx_h34d5h0t_420_xXx` will change your name to something stupid.
  
  * `;join Tracks of the World` will join the room called "Tracks of the
    World", creating it if it doesn't exist. (Creation of a room also sets
    the creator as that room's "Operator", although this currently bestows
    no special priviliges.)

  * `;priv somedude Come join tracksoftheworld.` will send the message
    "Come join tracksoftheworld" to the user whose name matches `somedude`
    (if that user exists).
    
  * `;who xxx` will request a list of all connected users whose names start
    with a case-and-whitespace-insensitive match of `xxx`. A plain `;who`
    with no text to match will return a list of all users.

  * `;rooms xxx` will, like `;who` above, request a list of all extant Room
    names that begin with a case-and-whitespace-insensitive match of `xxx`.
    A plain `;rooms` with no text to match will return a list of all Rooms.

### Server Instructions

The server configuration on my machine is at `~/.config/greld/greld.toml`;
it will probably be similarly placed on yours. You should make its contents
look something like this:

```toml
address = '192.168.1.13:51516'
tick_ms = 500
blackout_to_ping_ms  = 10000
blackout_to_kick_ms  = 20000
max_user_name_length = 24
max_room_name_length = 32
log_file = 'greld.log'
log_level = 1
```

although you may want to change the `address` value to match where you want
your server to bind.

You will want to run it with `nohup` if you don't want to babysit it:

```sh
you@your_machine:~/grel $ nohup target/release/greld &
```

and you may want to redirect `stdout` to a specific file.

### TODO (server):

  * Rate-limiting. There is some provision for rate-limiting built into
    the `User` struct, but currently the server does nothing with it.

  * ~~Users should be able to send private messages to each other.~~ done
    2021-01-03

  * Users should be able to "block" specific other users and not see their
    messages

  * Room ("channel" in the classic IRC sense) operators should be able to
    exercise certain regulatory influence over their Rooms, like muting
    or ejecting specific users.

  * ~~Users should be able to query the server for a full list/pattern-matching
    list of Room names.~~ done 2021-01-10

  * Eventually, I would like things like blocks/mutes/bans to be IP-specific,
    but that will require saving more state, and interacting more heavily
    with socket addresses.

### TODO (client):

  * ~~Spawning a new thread solely to catch `SIGWINCH` seems overkill. I don't
    know what I was thinking. I'm just going to have the client check the
    terminal size every time through its loop to detect changes.~~
    done 2020-12-30

  * ~~Switch client from using the excellent
    [`termion`](https://github.com/redox-os/termion) to the cross-platform
    [`crossterm`](https://github.com/crossterm-rs/crossterm) crate.~~ done
    2021-01-10, and now the client builds and runs on Windows

  * ~~The client should respond to `;who PARTIAL_NAME` input with the appropriate
    request (and then display the response properly).~~ done 2021-01-10; also
    both the server and client behave appropriately to the analagous
    `;rooms PARTIAL_NAME` request to list matching room names. The client
    doesn't have a specialized way to display these, though; it just displays
    the `Msg::Misc.alt` response.

  * A bunch of command-mode functionality needs to be implemented, like
    scrolling the various panes and resizing the roster window. (Some of this
    is done; some isn't.)
    
  * The configuration of the client should involve user-customizable colors.

I am happy to entertain feature requests, but simplicity is a goal.

