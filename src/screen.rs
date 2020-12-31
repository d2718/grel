/*! screen.rs

The `grel` client terminal output manager.

2020-12-30
*/

use lazy_static::lazy_static;
use log::{trace};
use std::io::{Write, stdout};
use termion::raw::RawTerminal;
use termion::{cursor, clear};

use super::line::*;

const SPACE: char = ' ';
const VBAR:  char = '│';
const HBAR:  char = '—';

lazy_static!{
    static ref DEFAULT_BORDER_BG: Option<BgCol> = None;
    static ref DEFAULT_BORDER_FG: Option<FgCol> = Some(FgCol::new(1, 1, 1));
    static ref DEFAULT_HIGH_BG:   Option<BgCol> = None;
    static ref DEFAULT_HIGH_FG:   Option<FgCol> = Some(FgCol::new(5, 5, 5));
    static ref VBARSTR: String = {
        let mut s = String::new();
        s.push(VBAR);
        s
    };
    
    static ref RESET_ALL: String = format!("{}{}{}",
        termion::color::Fg(termion::color::Reset),
        termion::color::Bg(termion::color::Reset),
        termion::style::Reset);
}

struct Bits {
    stat_begin: String,
    stat_begin_chars: usize,
    stat_end: String,
    stat_end_chars: usize,
    full_hline: String,
}

pub struct Screen {
    lines: Vec<Line>,
    input: Vec<char>,
    input_ip: u16,
    roster: Vec<Line>,
    roster_width: u16,
    stat_ul: Line,
    stat_ur: Line,
    stat_ll: Line,
    stat_lr: Line,
    lines_dirty: bool,
    input_dirty: bool,
    roster_dirty: bool,
    stat_dirty: bool,
    borders_bg: Option<BgCol>,
    borders_fg: Option<FgCol>,
    highlight_bg: Option<BgCol>,
    highlight_fg: Option<FgCol>,
    bits: Bits,
    
    lines_scroll: u16,
    lines_scroll_line_n: usize,
    roster_scroll: u16,
    last_x_size: u16,
    last_y_size: u16,
}

impl Screen {
    pub fn new<T: Write>(term: &mut RawTerminal<T>, roster_chars: u16) -> Screen {
        let (x, y): (u16, u16) = termion::terminal_size().unwrap();
        write!(term, "{}", cursor::Hide).unwrap();
        
        let new_bits = {
            let mut start = Line::new();
            let mut end = Line::new();
            start.pushf(&VBARSTR, DEFAULT_BORDER_FG.as_ref(),
                                  DEFAULT_BORDER_BG.as_ref(),
                                  Style::None);
            start.push(" ");
            end.push(" ");
            end.pushf(&VBARSTR, DEFAULT_BORDER_FG.as_ref(),
                                DEFAULT_BORDER_BG.as_ref(),
                                Style::None);
            let mut hline = Line::new();
            {
                let mut s = String::with_capacity(x as usize);
                for _ in 0..x { s.push(HBAR); }
                hline.pushf(&s, DEFAULT_BORDER_FG.as_ref(),
                                DEFAULT_BORDER_BG.as_ref(),
                                Style::None);
            }
            
            let start_len = start.len();
            let end_len = end.len();
            
            Bits {
                stat_begin: start.first_n_chars(start_len),
                stat_begin_chars: start_len,
                stat_end: end.first_n_chars(end_len),
                stat_end_chars: end_len,
                full_hline: hline.first_n_chars((x+1) as usize),
            }
        };
        
        Screen {
            lines: Vec::new(), input: Vec::new(), roster: Vec::new(),
            roster_width: roster_chars, input_ip: 0,
            stat_ul: Line::new(), stat_ur: Line::new(),
            stat_ll: Line::new(), stat_lr: Line::new(),
            lines_dirty: true,  input_dirty: true,
            roster_dirty: true, stat_dirty: true,
            borders_bg:   DEFAULT_BORDER_BG.clone(),
            borders_fg:   DEFAULT_BORDER_FG.clone(),
            highlight_bg: DEFAULT_HIGH_BG.clone(),
            highlight_fg: DEFAULT_HIGH_FG.clone(),
            lines_scroll: 0, roster_scroll: 0,
            lines_scroll_line_n: 0,
            last_x_size: x, last_y_size: y,
            bits: new_bits,
        }
    }
    
    pub fn bbg(&self) -> Option<&BgCol> { self.borders_bg.as_ref() }
    pub fn bfg(&self) -> Option<&FgCol> { self.borders_fg.as_ref() }
    pub fn hbg(&self) -> Option<&BgCol> { self.highlight_bg.as_ref() }
    pub fn hfg(&self) -> Option<&FgCol> { self.highlight_fg.as_ref() }
    
    /** Return the height of the main scrollback window. */
    pub fn get_main_height(&self) -> u16 { self.last_y_size - 2 }
    
    /** Return the number of `Line`s in the scrollback buffer. */
    pub fn get_scrollback_length(&self) -> usize { self.lines.len() }
    
    /** Trim the scrollback buffer to the latest `n` lines. */
    pub fn prune_scrollback(&mut self, n: usize) {
        if n >= self.lines.len() { return; }
        
        let mut temp: Vec<Line> = Vec::with_capacity(n);
        for _ in 0..n { temp.push(self.lines.pop().unwrap()); }
        self.lines = temp.drain(..).rev().collect();
        
        self.lines_dirty = true;
    }
    
    /** Push the supplied line onto the end of the scrollback buffer. */
    pub fn push_line(&mut self, l: Line) {
        self.lines.push(l);
        self.lines_dirty = true;
    }
    
    /** Populate the roster with the given slice of strings. */
    pub fn set_roster<T: AsRef<str>>(&mut self, items: &[T]) {
        self.roster = Vec::new();
        for s in items.iter() {
            let mut l: Line = Line::new();
            l.push(s.as_ref());
            self.roster.push(l);
        }
        self.roster_dirty = true;
    }
    
    /** Get number of characters in the input line. */
    pub fn get_input_length(&self) -> usize { self.input.len() }
    
    /** Add a `char` to the input line. */
    pub fn input_char(&mut self, ch: char) {
        if (self.input_ip as usize) >= self.input.len() {
            self.input.push(ch);
            self.input_ip = self.input.len() as u16;
        } else {
            self.input.insert(self.input_ip as usize, ch);
            self.input_ip += 1;
        }
        self.input_dirty = true;
    }
    
    /** Delete the character on the input line before the cursor.
    
    Obviously, this does nothing if the cursor is at the beginning.
    */
    pub fn input_backspace(&mut self) {
        let ilen = self.input.len() as u16;
        if ilen == 0 || self.input_ip == 0 { return; }
        
        if self.input_ip >= ilen {
            let _ = self.input.pop();
            self.input_ip = ilen - 1;
        } else {
            self.input_ip -= 1;
            let _ = self.input.remove(self.input_ip as usize);
        }
        self.input_dirty = true;
    }
    
    /** Delete the character on the input line _at_ the cursor.
    
    Obviously, this does nothing if the cursor is at the end.
    */
    pub fn input_delete(&mut self) {
        let ilen = self.input.len() as u16;
        if ilen == 0 || self.input_ip >= ilen { return; }
        
        let _ = self.input.remove(self.input_ip as usize);
        self.input_dirty = true;
    }
    
    /** Move the input cursor forward (or backward, for negative values)
    `n_chars`, or to the end (or beginning), if the new position would
    be out of range.
    */
    pub fn input_skip_chars(&mut self, n_chars: i16) {
        let cur = self.input_ip as i16;
        let new = cur + n_chars;
        if new < 0 {
            self.input_ip = 0;
        } else {
            let new: u16 = new as u16;
            let ilen = self.input.len() as u16;
            if new > ilen {
                self.input_ip = ilen;
            } else {
                self.input_ip = new;
            }
        }
        self.input_dirty = true;
    }
    
    /** Scroll the main display up (or down, for negative values) `n_chars`,
    or to the end (or beginning), if the new position would be out of range.
    */
    pub fn scroll_lines(&mut self, n_chars: i16) {
        let cur = self.lines_scroll as i16;
        let mut new = cur + n_chars;
        if new < 0 { new = 0; }
        self.lines_scroll = new as u16;
        self.lines_dirty = true;
    }
    
    /** Return the contents of the input line as a String and clear
    the input line.
    */
    pub fn pop_input(&mut self) -> Vec<char> {
        let mut new_v: Vec<char> = Vec::new();
        std::mem::swap(&mut new_v, &mut self.input);
        self.input_ip = 0;
        self.input_dirty = true;
        return new_v;
    }
    
    pub fn set_stat_ll(&mut self, new_stat: Line) {
        self.stat_ll = new_stat;
        self.stat_dirty = true;
    }
    pub fn set_stat_ul(&mut self, new_stat: Line) {
        self.stat_ul = new_stat;
        self.stat_dirty = true;
    }
    pub fn set_stat_ur(&mut self, new_stat: Line) {
        self.stat_ur = new_stat;
        self.stat_dirty = true;
    }
    
    /** Set the size at which the `Screen` should be rendered. This is
    intended to be the entire terminal window.
    
    If the terminal changes size, this should be called before the next
    call to `.refresh()`, or it probably won't look right.
    */
    pub fn resize(&mut self, cols: u16, rows: u16) {
        if cols != self.last_x_size {
            let mut s = String::with_capacity(cols as usize);
            for _ in 0..cols { s.push(HBAR); }
            let mut hl = Line::new();
            hl.pushf(&s, self.bfg(), self.bbg(), Style::None);
            self.bits.full_hline = hl.first_n_chars(cols as usize);
        }
        if (cols != self.last_x_size) || (rows != self.last_y_size) {
            self.lines_dirty = true;
            self.input_dirty = true;
            self.roster_dirty = true;
            self.stat_dirty = true;
            self.last_x_size = cols;
            self.last_y_size = rows;
        }
    }
    
    /** Automatically set the size of the `Screen` to be the whole
    terminal window.
    */
    pub fn auto_resize(&mut self) {
        let (x, y): (u16, u16) = termion::terminal_size().unwrap();
        self.resize(x, y);
    }
    
    fn refresh_lines<T: Write>(&mut self, term: &mut RawTerminal<T>,
                     width: u16, height: u16) {
        trace!("Screen::refresh_lines(..., {}, {}) called", &width, &height);
        let blank: String = {
            let mut s = String::new();
            for _ in 0..width { s.push(SPACE); }
            s
        };
        let mut y = height;
        let w = width as usize;
        let mut count_back: u16 = 0;
        for aline in self.lines.iter_mut().rev() {
            for row in aline.lines(w).iter().rev() {
                if y == 1 { break; }
                if count_back >= self.lines_scroll {
                    write!(term, "{}{}\r{}", cursor::Goto(1, y), &blank, &row)
                        .unwrap();
                    y -= 1;
                }
                count_back += 1;
            }
            if y == 1 { break; }
        }
        
        /* Check to see if we've scrolled past the end of the scrollback,
        and if so, scroll us forward a little bit and keep
        `self.lines_dirty == true` */
        if y > 1 && self.lines_scroll > 0 {
            let adjust: i16 = (y - 1) as i16;
            self.scroll_lines(-adjust);
        } else {
            while y > 1 {
                write!(term, "{}{}", cursor::Goto(1, y), &blank).unwrap();
                y -= 1;
            }
            self.lines_dirty = false;
        }
    }
    
    fn refresh_roster<T: Write>(&mut self, term: &mut RawTerminal<T>,
                      xstart: u16, height: u16) {
        trace!("Screen::refresh_roster(..., {}, {}) called", &xstart, &height);
        let rrw: usize = (self.roster_width as usize) + 1;
        
        let blank: String = {
            let mut s = String::new();
            for _ in 0..self.roster_width { s.push(SPACE); }
            let mut l = Line::new();
            l.pushf(&VBARSTR, self.bfg(), self.bbg(), Style::None);
            l.push(&s);
            l.first_n_chars(rrw)
        };
        let mut y: u16 = 2;
        let us_scroll = self.roster_scroll as usize;
        for (i, aline) in self.roster.iter().enumerate() {
            if y == height { break; }
            if i >= us_scroll {
                write!(term, "{}{}{}{}", cursor::Goto(xstart, y),
                       &blank, cursor::Goto(xstart+1, y),
                       aline.first_n_chars(self.roster_width as usize))
                    .unwrap();
                y += 1;
            }
        }
        while y <= height {
            write!(term, "{}{}", cursor::Goto(xstart, y), &blank).unwrap();
            y += 1;
        }
        self.roster_dirty = false;
    }
    
    fn refresh_input<T: Write>(&mut self, term: &mut RawTerminal<T>) {
        write!(term, "{}{}{}", cursor::Goto(1, self.last_y_size),
                               clear::CurrentLine,
                               cursor::Goto(1, self.last_y_size)).unwrap();
        let third = self.last_x_size / 3;
        let maxpos = self.last_x_size - third;
        let startpos = {
            if self.input.len() < self.last_x_size as usize {
                0
            } else if self.input_ip < third {
                0
            } else if self.input_ip > maxpos {
                self.input_ip - maxpos
            } else {
                self.input_ip - third
            }
        };
        let endpos = {
            if startpos + self.last_x_size > (self.input.len() as u16) {
                self.input.len() as u16
            } else {
                startpos + self.last_x_size
            }
        };
        
        trace!("Screen::refresh_input(): (startpos, endpos) = ({}, {})",
                startpos, endpos);
        
        let input_ip_us = self.input_ip as usize;
        for i in (startpos as usize)..(endpos as usize) {
            let c = self.input[i];
            if i == input_ip_us {
                write!(term, "{}{}{}", termion::style::Invert,
                                       c, termion::style::Reset).unwrap();
            } else {
                write!(term, "{}", c).unwrap();
            }
        }
        if input_ip_us == self.input.len() {
            write!(term, "{}{}{}", termion::style::Invert, SPACE,
                                   termion::style::Reset).unwrap();
        }
        
        self.input_dirty = false;
    }
    
    fn refresh_stat<T: Write>(&mut self, term: &mut RawTerminal<T>) {
        trace!("Screen::refresh_stat(...) called");
        
        /* Lower left corner (there is no lower-right as of yet). */
        let stat_pad = 2 + self.bits.stat_begin_chars + self.bits.stat_end_chars;
        let stat_room = (self.last_x_size as usize) - stat_pad;
        write!(term, "{}{}{}{}{}{}", cursor::Goto(1, self.last_y_size-1),
                     &self.bits.full_hline,
                     cursor::Goto(2, self.last_y_size-1),
                     &self.bits.stat_begin,
                     &self.stat_ll.first_n_chars(stat_room),
                     &self.bits.stat_end).unwrap();
        
        /* Upper left and right corners. */
        
        /* characters surrounding actual text:
        1    2    3
        -|  |-|  |-
        + 2 starts, 2 ends
        */
        let bits_sum = (3 + (self.bits.stat_begin_chars * 2)
                          + (self.bits.stat_end_chars * 2)) as u16;
        let tot_space = self.last_x_size - bits_sum;
        let space_each: usize = (tot_space / 2) as usize;
        let abbrev_space = space_each - 3;
        
        write!(term, "{}{}{}", cursor::Goto(1, 1), &self.bits.full_hline,
                               cursor::Goto(2, 1)).unwrap();
        if self.stat_ul.len() > space_each {
            write!(term, "{}{}...{}", &self.bits.stat_begin,
                         &self.stat_ul.first_n_chars(abbrev_space),
                         &self.bits.stat_end).unwrap();
        } else {
            write!(term, "{}{}{}", &self.bits.stat_begin,
                         &self.stat_ul.first_n_chars(space_each),
                         &self.bits.stat_end).unwrap();
        }
        
        if self.stat_ur.len() > space_each {
            let ur_offs: u16 = self.last_x_size -
                              (1 + self.bits.stat_begin_chars +
                                    self.bits.stat_end_chars +
                                    space_each) as u16;
            write!(term, "{}{}{}...{}", cursor::Goto(ur_offs, 1),
                         &self.bits.stat_begin,
                         &self.stat_ur.first_n_chars(abbrev_space),
                         &self.bits.stat_end).unwrap();
        } else {
            let ur_offs: u16 = self.last_x_size -
                              (1 + self.bits.stat_begin_chars +
                                    self.bits.stat_end_chars +
                                    self.stat_ur.len()) as u16;
            write!(term, "{}{}{}{}", cursor::Goto(ur_offs, 1),
                        &self.bits.stat_begin,
                        &self.stat_ur.first_n_chars(space_each),
                        &self.bits.stat_end).unwrap();
        }
        
        self.stat_dirty = false;
    }
    
    /** Redraw any parts of the `Screen` that have changed since the last
    call to `.refresh()`.
    */
    pub fn refresh<T: Write>(&mut self, term: &mut RawTerminal<T>) {
        //trace!("Screen::refresh(...) called");
        if !(self.lines_dirty  || self.input_dirty ||
                                 self.roster_dirty || self.stat_dirty) {
            return;
        }
        
        let rost_w = self.roster_width + 1;
        let main_w = self.last_x_size - rost_w;
        let main_h = self.last_y_size - 2;
        
        if (main_w < 20) || (main_h < 5) {
            write!(term, "{}{}The terminal window is too small. Please make it larger.",
                    clear::All, cursor::Goto(1, 1)).unwrap();
            term.flush().unwrap();
            return;
        }
        
        if self.input_dirty {
            self.refresh_input(term);
            /* Each of these resetting write!s kind of a hack. */
            write!(term, "{}", RESET_ALL.as_str()).unwrap();
        }
        if self.lines_dirty {
            self.refresh_lines(term, main_w, main_h);
            write!(term, "{}", RESET_ALL.as_str()).unwrap();
        }
        if self.roster_dirty {
            self.refresh_roster(term, main_w+1, main_h);
            write!(term, "{}", RESET_ALL.as_str()).unwrap();
        }
        if self.stat_dirty {
            self.refresh_stat(term);
            write!(term, "{}", RESET_ALL.as_str()).unwrap();
        }
        term.flush().unwrap();
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let mut stdout = stdout();
        write!(stdout, "{}{}\n", cursor::Show, clear::All).unwrap();
        stdout.flush().unwrap();
    }
}
