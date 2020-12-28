#!/usr/bin/env lua

--[[
lcwex.lua

Lua Curses Window EXample (EXploration?)

2020-12-16
--]]

local curses = require 'curses'

local stdscr = nil
local window_props = {}

local function init()
    stdscr = curses.initscr()
    curses.echo(false)
    curses.nl(false)
    curses.cbreak(true)
    curses.curs_set(0)
    stdscr:keypad(true)
    --stdscr:timeout(100)
    stdscr:clear()
end

local function make_line(w)
    local n = 0
    local d = 0
    local ctab = {}
    while n < w do
        table.insert(ctab, string.format("%d", d))
        d = d + 1
        if d > 9 then d = 0 end
        n = n + 1
    end
    return table.concat(ctab, '')
end

local function main()
    init()
    
    local ncols = curses.cols()
    local nlins = curses.lines()
    local count_line = make_line(ncols)
    
    for y = 0,nlins-1 do
        stdscr:mvaddstr(y, 0, count_line)
    end
    
    local w = stdscr:derive(25, 25, 5, 5)
    w:erase()
    w:border()
    w:mvaddch(1, 1, 'X')
    local y, x = w:getmaxyx()
    w:mvaddstr(2, 1, string.format('%d, %d', y, x))
    stdscr:refresh()
    
    for k, _ in pairs(getmetatable(w)) do table.insert(window_props, k) end
    
    stdscr:getch()
    curses.endwin()
end

local function err(e)
    curses.endwin()
    print("Error during curses session:")
    print(debug.traceback(e, 2))
    os.exit(1)
end

xpcall(main, err)

table.sort(window_props)
for _, v in ipairs(window_props) do print(v) end
