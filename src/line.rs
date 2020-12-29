/*!
line.rs

text wrapping for the `grel` terminal client

2020-12-24

The major functionality is text wrapping, but it may evolve into regulating
all of the screen-drawing functions.

As of 2020-12-23, there is quit a bit of copying and reallocating,
particularly of small strings containing ANSI formatting codes. This may
change in the future as I get more comfortable reasoning about lifetimes.
*/
use lazy_static::lazy_static;
use log::debug;

lazy_static! {
    static ref NORMAL_FG: String = {
        use termion::color::{Fg, Reset};
        format!("{}", Fg(Reset))
    };
    
    static ref NORMAL_BG: String = {
        use termion::color::{Bg, Reset};
        format!("{}", Bg(Reset))
    };
    
    static ref BOLD_STYLE:              String
        = format!("{}", termion::style::Bold);
    static ref INVERT_STYLE:            String
        = format!("{}", termion::style::Invert);
    static ref UNDERLINE_STYLE:         String
        = format!("{}", termion::style::Underline);
    static ref BOLD_UNDERLINE_STYLE:    String
        = format!("{}{}", termion::style::Bold, termion::style::Underline);
    static ref INVERT_UNDERLINE_STYLE:  String
        = format!("{}{}", termion::style::Invert, termion::style::Underline);
    
    static ref NORMAL_STYLE: String = format!("{}", termion::style::Reset);
}

/** `FgCol` and `BgCol` represent arbitrary ANSI foreground and background
colors. They are used in the `Line::pushf(...)` method (below) to specify
that a pushed chunk should be rendered with a particular color.
*/
#[derive(Clone)]
pub struct FgCol(String);
#[derive(Clone)]
pub struct BgCol(String);

impl FgCol {
    /** Creates a new foreground color specification object. **Will
    panic** if `r`, `g`, or `b` is greater than 5.
    */
    pub fn new(r: u8, g: u8, b: u8) -> FgCol {
        if r > 5 || g > 5 || b > 5 {
            panic!("FgCol::new(): color values must be 5 or less");
        }
        
        let av = termion::color::AnsiValue::rgb(r, g, b);
        FgCol(format!("{}", termion::color::Fg(av)))
    }
}

impl BgCol {
    /** Creates a new background color specification object. **Will
    panic** if `r`, `g`, or `b` is greater than 5.
    */
    pub fn new(r: u8, g: u8, b: u8) -> BgCol {
        if r > 5 || g > 5 || b > 5 {
            panic!("BgCol::new(): color values must be 5 or less");
        }
        
        let av = termion::color::AnsiValue::rgb(r, g, b);
        BgCol(format!("{}", termion::color::Bg(av)))
    }
}

/** Specifies a style; for passing to `Line::pushf(...)`. */
#[derive(Eq, PartialEq, Clone, Copy)]
pub enum Style {
    None,
    Bold,
    Invert,
    Underline,
    BoldUnderline,
    InvertUnderline,
}

impl Style {
    /** `Line::wrap(...)` uses this write the appropriate style escape codes. */
    fn bytes(&self) -> &str {
        match self {
            Style::None => { "" },
            Style::Bold => { &BOLD_STYLE },
            Style::Invert => { &INVERT_STYLE },
            Style::Underline => { &UNDERLINE_STYLE },
            Style::BoldUnderline => { &BOLD_UNDERLINE_STYLE },
            Style::InvertUnderline => { &INVERT_UNDERLINE_STYLE },
        }
    }
}

/** Used by the `Line` internals to store formatting information. */
#[derive(Clone)]
struct Fmtr{
    idx: usize,
    code: String,
}

impl Fmtr {
    fn new(i: usize, from: &str) -> Fmtr {
        Fmtr { idx: i, code: String::from(from), }
    }
}

/** The `Line` represents a single line of text, possibly with color and
style formatting information. It can be built up by having chunks of text
pushed onto the end of it. You can then request a rendering of it at different
wrapping widths.

```
use grel::line::{BgCol, FgCol, Style, Line};

let blue = BgCol::new(0, 0, 2);
let cyan = FgCol::new(0, 5, 5);
let none = Style::None;         // This makes calls to pushf() look nicer.

let mut x = Line::new();
// Note careful surrounding spacing throughout.
x.push("This sentence is unformatted. ");
x.pushf("This is colored.", Some(&cyan), Some(&blue), none);
x.push(" This sentence has ");
x.pushf("two words", None, None, Style::Bold);
x.push(" that are emphasized. Multiple types of simultaneous formatting ");
x.push("can be distracting, so use them ");
x.pushf("SpArInGlY", Some(&cyan), Some(&blue), Style::InvertUnderline);
x.push("!!! (Just like you would multiple exclamation points.)");

for _ in 0..36 { print!("-"); }
println!("");
for line in x.lines(36).iter() { println!("{}", line); }
for _ in 0..60 { print!("-"); }
println!("");
for line in x.lines(60).iter() { println!("{}", line); }

```

*/
pub struct Line {
    chars:  Vec<char>,
    width:  Option<usize>,
    fdirs:  Vec<Fmtr>,
    render: Vec<String>,
}

impl Line {
    /** Create a new, empty line. */
    pub fn new() -> Line {
        Line {
            chars: Vec::new(),
            width: None,
            fdirs: Vec::new(),
            render: Vec::new(),
        }
    }
    
    /** Return the length of the line in `char`s.
    
    This may not be the same as the number of characters that actually
    end up when `.wrap(n)`ped, because leading spaces on lines after the
    first are skipped.
    */
    pub fn len(&self) -> usize { self.chars.len() }
    
    /** Add a chunk of unformatted text to the end of a `Line`. */
    pub fn push(&mut self, s: &str) {
        self.width = None;
        for c in s.chars() { self.chars.push(c); }
    }
    
    /** Add a chunk of _formatted_ text to the end of a `Line`. See
    `FgCol` and `Style` above for details on how to specify the formatting.
    */
    pub fn pushf(
        &mut self,
        s: &str,
        fg: Option<&FgCol>,
        bg: Option<&BgCol>,
        style: Style
    ) {
        self.width = None;
        let mut n: usize = self.chars.len();
        
        if let Some(fgc) = fg {
            self.fdirs.push(Fmtr::new(n, &fgc.0));
        }
        if let Some(bgc) = bg {
            self.fdirs.push(Fmtr::new(n, &bgc.0));
        }
        if style != Style::None {
            self.fdirs.push(Fmtr::new(n, style.bytes()));
        }
        
        for c in s.chars() { self.chars.push(c); }
        n = self.chars.len();
        
        if style != Style::None {
            self.fdirs.push(Fmtr::new(n, &NORMAL_STYLE));
        }
        if let Some(_ ) = bg {
            self.fdirs.push(Fmtr::new(n, &NORMAL_BG));
        }
        if let Some(_) = fg {
            self.fdirs.push(Fmtr::new(n, &NORMAL_FG));
        }
    }
    
    pub fn append(&mut self, other: &Self) {
        let base = self.chars.len();
        for c in other.chars.iter() { self.chars.push(*c); }
        for f in other.fdirs.iter() {
            self.fdirs.push(Fmtr::new(base + f.idx, &(f.code)));
        }
    }
    
    /** Wrap the text to fit horizontally in a space `tgt` characters wide.
    Let's just say it's UB if you call it with something stupid like 0.
    
    Figuring out (and then implementing) how to make ANSI-formatted text work
    was the hardest part of the project. There are probably easier ways to
    do this (like, uh, every WYSIWYG word processor). A couple of approaches
    I didn't want to take:
    
     * Tagging each character with style information and writing
       style-on/style-off codes for _each_ character. While the performance
       hit on even semi-modern hardware would probably be unnoticeable, I
       felt this was unnecessarily lazy, and I could do better.
       
     * Tagging each character with style information and then doing analysis
       during the write to determine which styles are on/off/switching and
       writing codes based on that. That seemed like it was approaching
       a reimplementation of curses, and since we're already purposefully
       _not_ using curses, why use it? Also, it seemed like it required a
       lot of state and logic during the write algorithm.
    
    So here's what we have:
    
    The raw text is stored as a vector of `char`s;
    formatting information is generated during calls to `.pushf()` and stored
    as a vector of `Fmtr` structs, each of which contains the index of the
    `char` `Vec` where it applies, and the `String` with the formatting
    code that goes along with the format change.
    
    When `.wrap(n)` is called, a first pass through the `char`s determines
    where the line breaks will be, and a second pass through writes a
    vector of `String`s (one per wrapped line) complete with the formatting
    information.
    
    A reference to this "rendered" `Vec<String>` is returned by the `.lines(n)`
    function (below); each line suitable for printing directly to the
    terminal window.
    */
    fn wrap(&mut self, tgt: usize) {
        debug!("Line::wrap({}) called", &tgt);
        debug!("    chars: {}", &(self.chars.iter().collect::<String>()));
            
        let mut wraps: Vec<usize> = Vec::with_capacity(1 + self.chars.len() / tgt);
        let mut x: usize = 0;
        let mut lws: usize = 0;
        let mut write_leading_ws: bool = true;
        
        for (i, c) in self.chars.iter().enumerate() {
            if x == tgt {
                if i - tgt >= lws {
                    wraps.push(i);
                    x = 0;
                } else {
                    wraps.push(lws);
                    x = i - lws;
                }
                write_leading_ws = false;
            }
            if c.is_whitespace() {
                lws = i;
                if x > 0 || write_leading_ws {
                    x = x + 1;
                }
            } else {
                x = x + 1;
            }
        }
        
        debug!("    wrapping at: {:?}", &wraps);
        
        self.render = Vec::with_capacity(wraps.len());
        let mut fmt_iter = self.fdirs.iter();
        let mut nextf = fmt_iter.next();
        let mut cur_line = String::with_capacity(tgt);
        write_leading_ws = true;
        let mut wrap_idx: usize = 0;
        let mut line_len: usize = 0;
        
        for(i, c) in self.chars.iter().enumerate() {
            if wrap_idx < wraps.len() {
                if wraps[wrap_idx] == i {
                    debug!("    pushing current line: {}", &cur_line);
                    self.render.push(cur_line);
                    cur_line = String::with_capacity(tgt);
                    write_leading_ws = false;
                    wrap_idx = wrap_idx + 1;
                    line_len = 0;
                }
            }
            
            while match nextf {
                None => false,
                Some(f) => {
                    if f.idx == i {
                        debug!("    pushing formatting code at {}: {:?}",
                                i, &f.code.as_bytes());
                        cur_line.push_str(&f.code);
                        nextf = fmt_iter.next();
                        true
                    } else {
                        false
                    }
                },
            } {}
            
            if line_len > 0 {
                cur_line.push(*c);
                line_len = line_len + 1;
            } else if write_leading_ws || !c.is_whitespace() {
                cur_line.push(*c);
                line_len = line_len + 1;
            }
        }
        
        while let Some(f) = fmt_iter.next() {
            debug!("    appending terminal formatting codes: {:?}",
                    &f.code.as_bytes());
            cur_line.push_str(&f.code);
        }
        
        self.render.push(cur_line);
        
        self.width = Some(tgt);
    }
    
    /** Return a reference to the lines of the stored text, wrapped to `width`
    characters. If the text has not been last wrapped to `width`, this will
    require rewrapping and rerendering it, which is why this is `&mut self`.
    */
    pub fn lines(&mut self, width: usize) -> &[String] {
        match self.width {
            None => { self.wrap(width); },
            Some(n) => {
                if n != width { self.wrap(width); }
            }
        }
        
        return &self.render;
    }
    
    pub fn first_n_chars(&self, n: usize) -> String {
        let tgt_n: usize = match n < self.chars.len() {
            true => n,
            false => self.chars.len(),
        };
        
        let mut s = String::new();
        let mut fmt_iter = self.fdirs.iter();
        let mut nextf = fmt_iter.next();
        for (i, c) in self.chars[..tgt_n].iter().enumerate() {
            while match nextf {
                None => false,
                Some(f) => {
                    if f.idx == i {
                        s.push_str(&f.code);
                        nextf = fmt_iter.next();
                        true
                    } else {
                        false
                    }
                },
            } {}
            s.push(*c);
        }
        while let Some(f) = nextf {
            s.push_str(&f.code);
            nextf = fmt_iter.next();
        }
        
        return s;
    }
}

#[cfg(test)]
mod test {
    use super::*;
    
    const NEWLINE: char = '\n';
    const SPACE:   char = ' ';
    
    fn rnr(fname: &str) -> String {
        let mut s = std::fs::read_to_string(fname).unwrap();
        s = s.chars().map(|c| {
            match c {
                NEWLINE => SPACE,
                a @ _ => a,
            }
        }).collect();
        return s;
    }
    
    #[test]
    fn test_wrap() {
        for fname in &["test_strs/first.txt",
                        "test_strs/second.txt"] {
        
            let mut el: Line = Line::new();
            el.push(&rnr(fname));
        
            for n in &[40usize, 50, 60] {
                for _ in 0..*n { print!("-"); }
                println!("");
                for line in el.lines(*n) { println!("{}", &line); }
            }
        }
    }
    
    #[test]
    fn test_color() {
        let red_fg: FgCol = FgCol::new(5, 0, 0);
        let blue_bg: BgCol = BgCol::new(0, 0, 2);
        let mut x: Line = Line::new();
        x.pushf("xXx_HeAdShOt_xXx", Some(&red_fg), None, Style::None);
        x.push(": u r a noob. I ");
        x.pushf("keeeeeeld", None, None, Style::Bold);
        x.push(" joo! ");
        x.pushf("HARD!!!", None, Some(&blue_bg), Style::None);
        x.push(" and i think u r a luser. Looooo");
        x.pushf("0o0o0o0o0o0o0", None, None, Style::Invert);
        x.push("ozzzer...!!!");
        
        for n in &[40usize, 50, 60] {
            for _ in (0..*n) { print!("-"); }
            println!("");
            for line in x.lines(*n) { println!("{}", &line); }
        }
    }
}
