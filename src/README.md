# `cleanup` project

Issues:

### `process_room()`

~~The `process_room(...)` function in `greld.rs` runs from lines 116 to 860
(out of a 991-line source file, comments included). This should probably
get broken up into more manageable chunks. It's true that this function does
several _classes_ of things; the trouble is that there are some mutable
variables that are used throught the entire shebang. I have to find a
way to make this more modular in a way that makes sense.~~

This is preliminarily done on 2021-01-16. `process_room(...)` now runs
from lines 941 to 1084. `greld.rs` has grown to 1222 lines, but I _have_

  1. introduced a new function to specifically handle each of the `proto2::Msg`
     variants from the client,
  2. introduced a new structure (`Context`) to hold (and pass) all the mutable
     references that each of these functions needs, and
  3. written a lot of function calls in a style that takes more vertical space.

The `do_query(...)` and `do_op(...)` functions are both still pretty long,
but they do have to handle multiple different types of `Msg::Op`s and
`Msg::Query`s, respectively.

### `String` allocations

~~There are heap allocations everywhere that seem like they _should_ be
avoidable. For example, a chunk of code like this:~~

```rust
  let leave_msg = Msg::Misc {
      what: String::from("leave"),
      data: vec![String::from(u.get_name()),
                 "moved to another room".to_string()],
      alt: format!("{} moved to another room.", u.get_name()),
  };
  let leave_env = Env::new(Endpoint::User(*w), Endpoint::Room(rid), &leave_msg);
  envz.push(leave_env);
```

~~The creation of this `Msg::Misc{}` involved copying the static `"leave"`
into a heap-allocated `String`, cloning a user's name into another `String`,
copying another static ( `"moved to another room"` ) into yet another
`String`, and then formatting for the `.alt` member _also_ requires another
heap-allocated `String`. _Then_, immediately thereafter, all of that gets
serialized into a vector of bytes, and none of those allocations are
needed anymore. It seems like at least _some_ of that allocation could be
avoided.~~

~~Is it possible to have my structs take an `AsRef<str>` instead of a `String`
and still work/serialize correctly?~~

~~Maybe I need to have two different types of `proto2::Msg` structs that both
serialize to JSON identically, one that takes `&str`s and one that takes
owned `String`s. Of course, we will only ever _de_serialize the type that
takes owned `String`s.~~

UPDATE 2021-02-02: The `less_alloc` branch has replaced `proto2` with
`proto3`, which has two types of structs: `Sndr` structs which take their
values by reference (because they're just going to get serialiezd right
away anyway) and `Rcvr` structs, which own their values. `Sndr`s get
encoded in `Env`s and subsequently written to `Sock`s; `Rcvr`s get decoded
_from_ `Sock`s. Documentation and cleanup still required, but this seems to
work.

UPDATE 2020-02-05: In addition to using `&str`s wherever possible, I have
also replaced `Vec`s with arrays wherever possible, and introduced the
[`smallvec`](https://docs.rs/smallvec/1.6.1/smallvec/index.html)
crate and used `SmallVec`s in a lot of situations where fixed-sized
arrays weren't practical.

Of course, I haven't bothered to introduce a custom allocator to really
see if this makes a difference (neither `ptrace` or `valgrind`---even with
`massif`--have been particularly helpful, because it looks like Rust prefers
`brk()`ing generously and allocating itself rather than calling `malloc()`.)

### Client spaghetti

Client functionality is split up into several modules (`screen`, `line`,
`sock`, partially `config`), but there doesn't seem to be enough separation
of concerns, and they seem to all have their hands in each others' pockets.
For example, `crossterm` stuff leaks into several of them, _as well as_ the
`grel.rs` itself. The `config::Colors` struct ends up in the client; it seems
like this should belong to some other module, maybe.

It seems there is a need for additional modularization.

### Repetition

~~The client is full of things like~~

```rust
"name" => {
    if cmd_toks.len() > 2 { for s in &cmd_toks[2..] { arg.push_str(s); } }
    let b = Msg::Name(arg).bytes();
    gv.socket.enqueue(&b);
},

"join" => {
    if cmd_toks.len() > 2 { for s in &cmd_toks[2..] { arg.push_str(s); } }
    let b = Msg::Join(arg).bytes();
    gv.socket.enqueue(&b);
},

// ... five more nearly-identical chunks, and several other
// less-identical-but-similarly-patterned chunks ...
```

UPDATE 2021-01-17: I have done something about this particular section of
`respond_to_user_input()`. It's not any _shorter_ or _less repetitive_,
but it turns `if`s into `match`es and simplifies cases where there are
_sub_commands.

```rust
let name = match data.get(0) {
    None => { return Err(format!("Incomplete data: {:?}", &m)); },
    Some(x) => x,
};
let message = match data.get(1) {
    None => { return Err(format!("Incomplete data: {:?}", &m)); },
    Some(x) => x,
};

// ...plus several more `match` branches with the exact same pattern
// in them ...

```

UPDATE 2017-01-17: _Some_ of the above noise was reduced with a
destructuring slice match. The above code became

```rust
let (name, message) = match &data[..] => {
    [x, y] => (x, y),
    _ => { return Err(format!("Incomplete data: {:?}", &m)); },
};
```

which is nicer, at least in the case that the `data` vector of the
`Msg::Misc` variant has multiple elements.

UPDATE: As of 2021-01-17, the `Context` struct in the _server_ has had
some `impl` functions introduced to allow this kind of thing to be collapsed
into the `?` operator in certain places. The client still ~~needs~~ might
be able to use some work.
