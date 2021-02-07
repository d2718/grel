/*!
Formatted text wrapping library for `grel` terminal client

This new version uses the
[`crossterm`](https://github.com/crossterm-rs/crossterm)
library. (The old version used `termion`, which I likes a great deal, but
didn't support any Windows terminals.)

The major idea of the `Line` struct is to store formatted text in a format
that can be properly wrapped (and rewrapped) to fit different screen widths.

updated 2021-01-08
*/

use lazy_static::lazy_static;
use log::trace;

use crossterm::{ ExecutableCommand, style };

lazy_static! {
    static ref NOSTYLE: Style = {
        use crossterm::{ ExecutableCommand, style };
        let mut buff: Vec<u8> = Vec::new();
        let cols = style::Colors::new(style::Color::Reset, style::Color::Reset);
        buff.execute(style::SetColors(cols)).unwrap();
        buff.execute(style::SetAttribute(style::Attribute::Reset)).unwrap();
        Style(String::from_utf8(buff).unwrap())
    };
}

/** A `Style` is just a wrapper for a string containing the ANSI codes to
write text in a given style to the terminal.
*/
#[derive(Clone)]
pub struct Style(String);

impl Style {
    pub fn new(fg: Option<style::Color>,
               bg: Option<style::Color>,
               attrs: Option<&[style::Attribute]>
    ) -> Style {
        let mut buff = Vec::new();
        let cols = style::Colors{ foreground: fg, background: bg };
        buff.execute(style::SetColors(cols)).unwrap();
        if let Some(x) = attrs {
            for attr in x.iter() {
                buff.execute(style::SetAttribute(*attr)).unwrap();
            }
        }
        
        return Style(String::from_utf8(buff).unwrap());
    }
}

impl std::ops::Deref for Style {
    type Target = str;
    fn deref(&self) -> &Self::Target { &self.0 }
}

/** This struct is used in the `Line` internals to store formatting info. */
#[derive(Clone)]
struct Fmtr {
    idx: usize,
    code: Style,
}

impl Fmtr {
    fn new(i: usize, from: &Style) -> Fmtr {
        Fmtr { idx: i, code: from.clone(), }
    }
}

/** The `Line` represents a single line of text, possibly with color and style
formatting information. It can be built up by pushing chunks of text onto the
end of it. You can request a rendering of it at different wrapping widths.

```
use grel::ctline::{Style, Line};
use crossterm::style;

let blues = Style::new(Some(style::Color::Cyan),
                       Some(style::Color::DarkBlue),
                       None);
let bold  = Style::new(None, None, Some(&[style::Attribute::Bold]));
let splat = Style::new(Some(style::Color::Cyan),
                       Some(style::Color::DarkBlue),
                       Some(&[style::Attribute::Reverse,
                              style::Attribute::Underlined]));

let mut x = Line::new();
// Note careful surrounding spacing throughout.
x.push("This sentence is unformatted. ");
x.pushf("This is colored.", &blues);
x.push(" This sentence has ");
x.pushf("two words", &bold);
x.push(" that are emphasized. Multiple types of simultaneous formatting ");
x.push("can be distracting, so use them ");
x.pushf("SpArInGlY", &splat);
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
    chars: Vec<char>,
    width: Option<usize>,
    nchars: Option<usize>,
    fdirs: Vec<Fmtr>,
    render: Vec<String>,
    nchars_render: String,
}

impl Line {
    /** Instantiate a new, empty `Line`. */
    pub fn new() -> Line {
        Line {
            chars: Vec::new(),
            width: None,
            nchars: None,
            fdirs: Vec::new(),
            render: Vec::new(),
            nchars_render: String::new(),
        }
    }
    
    /** Return the number of characters in the `Line`. */
    pub fn len(&self) -> usize { self.chars.len() }
    
    /** Add a chunk of unformatted text to the end of the `Line`. */
    pub fn push<T: AsRef<str>>(&mut self, s: T) {
        self.width = None;
        self.nchars = None;
        for c in s.as_ref().chars() { self.chars.push(c); }
    }
    
    /** Add a chunk of _formatted_ text to the end of the `Line`. */
    pub fn pushf<T: AsRef<str>>(&mut self, s: T, styl: &Style) {
        self.width = None;
        self.nchars = None;
        
        let mut n: usize = self.chars.len();
        self.fdirs.push(Fmtr::new(n, styl));
        
        for c in s.as_ref().chars() { self.chars.push(c); }
        
        n =self.chars.len();
        self.fdirs.push(Fmtr::new(n, &NOSTYLE));
    }
    
    /** Append a copy of the contents of `other` to `self`. */
    pub fn append(&mut self, other: &Self) {
        self.width = None;
        self.nchars = None;
        
        let base = self.chars.len();
        for c in other.chars.iter() { self.chars.push(*c); }
        for f in other.fdirs.iter() {
            self.fdirs.push(Fmtr::new(base + f.idx, &f.code));
        }
    }
    
    fn wrap(&mut self, tgt: usize) {
        
        let mut wraps: Vec<usize> = Vec::with_capacity(1 + self.chars.len() / tgt);
        let mut x: usize = 0;
        let mut lws: usize = 0;
        let mut write_leading_ws: bool = true;
        
        trace!("chars: {}", &(self.chars.iter().collect::<String>()));
        
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
        
        trace!("wraps at: {:?}", &wraps);
        
        self.render = Vec::with_capacity(wraps.len() + 1);
        let mut fmt_iter = self.fdirs.iter();
        let mut nextf = fmt_iter.next();
        let mut cur_line = String::with_capacity(tgt);
        write_leading_ws = true;
        let mut wrap_idx: usize = 0;
        let mut line_len: usize = 0;
        
        for (i, c) in self.chars.iter().enumerate() {
            if wrap_idx < wraps.len() {
                if wraps[wrap_idx] == i {
                    self.render.push(cur_line);
                    cur_line = String::with_capacity(tgt);
                    write_leading_ws = false;
                    wrap_idx += 1;
                    line_len = 0;
                }
            }
            
            while match nextf {
                None => false,
                Some(f) => {
                    if f.idx == i {
                        cur_line.push_str(&f.code);
                        nextf = fmt_iter.next();
                        true
                    } else {
                        false
                    }
                },
            } {}
            
            if line_len > 0 || write_leading_ws || !c.is_whitespace() {
                cur_line.push(*c);
                line_len = line_len + 1;
            }
        }
        
        while let Some(f) = nextf {
            cur_line.push_str(&f.code);
            nextf = fmt_iter.next();
        }
        
        self.render.push(cur_line);
        
        self.width = Some(tgt);
    }
    
    pub fn lines(&mut self, width: usize) -> &[String] {
        match self.width {
            None => { self.wrap(width); },
            Some(n) => {
                if n != width { self.wrap(width); }
            },
        }
        
        return &self.render;
    }
    
    fn render_n_chars(&mut self, n: usize) {
        let mut s = String::new();
        let mut fmt_iter = self.fdirs.iter();
        let mut nextf = fmt_iter.next();
        
        for (i, c) in self.chars[..n].iter().enumerate() {
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
        
        self.nchars = Some(n);
        self.nchars_render = s;
    }
    
    pub fn first_n_chars(&mut self, n: usize) -> &str {
        let tgt = match n < self.chars.len() {
            true => n,
            false => self.chars.len(),
        };
        
        match self.nchars {
            None => { self.render_n_chars(tgt); },
            Some(i) => {
                if tgt != i { self.render_n_chars(tgt); }
            },
        }
        
        return &self.nchars_render;
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
        for fname in &["test_strs/first.txt", "test_strs/second.txt"] {
            let mut el: Line = Line::new();
            el.push(&rnr(fname));
            
            for n in &[36usize, 50, 60] {
                for _ in 0..*n { print!("-"); }
                println!("");
                for line in el.lines(*n) { println!("{}", &line); }
            }
        }
    }
    
    #[test]
    fn test_color() {
        use crossterm::style::{Color, Attribute};
        
        let red_fg   = Style::new(Some(Color::Red), None, None);
        let grey_bg  = Style::new(None, Some(Color::DarkGrey), None);
        let bold     = Style::new(None, None, Some(&[Attribute::Bold]));
        let inverted = Style::new(None, None, Some(&[Attribute::Reverse]));
        
        let mut x = Line::new();
        x.pushf("grel dude", &red_fg);
        x.push(": I say some dialog. This is an ");
        x.pushf("eeeeemphasized", &bold);
        x.push(" word. ");
        x.pushf("I mean it!!!", &grey_bg);
        x.push(" And I am not kidding around, dude. You are a loooo");
        x.pushf("0o0o0o0o0o0o0o0o0", &inverted);
        x.push("oooooser.");
        
        for n in &[36usize, 50, 60] {
            for _ in 0..*n { print!("-"); }
            println!("");
            for line in x.lines(*n) { println!("{}", &line); }
        }
    }
}
