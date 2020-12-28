#!/usr/bin/env lua

--[[ xtcli.lua

grel testing client in lua,
extended version with more features

2020-12-19
--]]

local DEBUG = false

require 'localizer'
local socket = require 'socket'
local curses = require 'curses'
local json   = require 'dkjson'
local argz   = require 'dargs'
local dfmt   = require 'dfmt'

--local ADDR = '127.0.0.1'
local ADDR = '192.168.1.13'
local PORT = 51516
-- Number of bytes to attempt to read on each read from the socket.
local READ_SIZE = 1024
local LOG_FILE = 'tcli.log'
local TICK_TIMEOUT = 100

-- Width of the room's roster bar at right.
local ROSTER_WIDTH = 24
local STATUS_WIDTH = 42

local SPACE = string.byte(' ')
local NEWLINE = 13

local uname = nil

local function panick(err)
    curses.endwin()
    print("Encountered an error during curses session:")
    print(debug.traceback(err, 2))
    os.exit(1)
end

local function dbglog(fmtstr, ...)
    if not DEBUG then return nil end
    local msg = string.format(fmtstr, unpack(arg))
    local timestamp = os.date('%Y-%m-%d %T ')
    local n = #msg
    f = io.open(LOG_FILE, 'a')
    f:write(timestamp)
    f:write(msg)
    if msg:sub(n, n) ~= '\n' then f:write('\n') end
    f:close()
end

local function window_debug(w)
    local y, x = w:getbegyx()
    local v, h = w:getmaxyx()
    dbglog('    (%d, %d) & (%d, %d)', y, x, v, h)
end

local sock = nil

local snd_buffer= ''

local function enqueue(json_t)
    local msg = json.encode(json_t)
    if snd_buffer:len() > 0 then
        snd_buffer = snd_buffer .. msg
    else
        snd_buffer = msg
    end
end

local function nudge(s)
    if snd_buffer:len() == 0 then return nil end
    
    local n, err = s:send(snd_buffer)
    if err then
        dbglog('nudge(): socket:send() returned error: "%s"', err)
        return err
    end
    dbglog('nudge(): sent up to byte %d', n)
    if n > 1 then
        local new_buff = string.sub(snd_buffer, n+1)
        snd_buffer = new_buff
    end
    return nil
end

local function blocking_send(s, msg_obj)
    enqueue(msg_obj)
    while snd_buffer:len() > 0 do
        local err = nudge(s)
        if err then return err end
    end
    return nil
end

local rcv_buffer = ''

local function suck_from_socket(s)
    local keep_sucking = true
    local bytes_read = 0
    local err_return = nil
    while keep_sucking do
        local whole, err, part = s:receive(READ_SIZE)
        if whole then
            bytes_read = bytes_read + READ_SIZE
            rcv_buffer = rcv_buffer .. whole
        elseif err == 'timeout' then
            local n = part:len()
            if n > 0 then
                bytes_read = bytes_read + n
                rcv_buffer = rcv_buffer .. part
            else
                keep_sucking = false
            end
        elseif err then
            local n = part:len()
            if n > 0 then
                bytes_read = bytes_read + n
                rcv_buffer = rcv_buffer .. part
            end
            keep_sucking = false
            err_return = err
        else
            dbglog('suck_from_socket(): SHOULDN\'T HAPPEN: whole, err both nil')
            err_return = 'socket:receive() resulted in an unusual condition'
            keep_sucking = false
        end
    end
    return bytes_read, err_return
end

local function try_read(s)
    local bytes_read, err = suck_from_socket(s)
    --dbglog('try_read(): got %d bytes', bytes_read)
    
    if bytes_read > 0 then
    dbglog('try_read(): input buffer is "%s"', rcv_buffer)
        local chunks = {}
        local keep_chunking = true
        while keep_chunking do
            local chunk, offs = json.decode(rcv_buffer)
            if chunk then
                dbglog('try_read(): got a chunk; buffer offest %d', offs)
                table.insert(chunks, chunk)
                local new_buff = string.sub(rcv_buffer, offs)
                rcv_buffer = new_buff
                
            else
                keep_chunking = false
            end
        end
        if #chunks > 0 then
            return chunks, err
        else
            return nil, err
        end
    end
    
    return nil, errr
end

local lines = {}

local function add_line(str)
    dbglog('add_line("%s") called', str)
    local words = {}
    for w in str:gmatch('%s*%S+') do table.insert(words, w) end
    local t = {
        ['words'] = words,
        ['width'] = -1,
        ['render'] = {},
    }
    
    table.insert(lines, 1, t)
end

local function wrap(t, width)
    dbglog('wrap({...}, %d) called', width)
    if width ~= t.width then
        local new_lines = {}
        local chunks = {}
        local idx = 0
        for _, word in ipairs(t.words) do
            local holdover_word = true
                while holdover_word do
                if #chunks == 0 then
                    if #new_lines > 0 then
                        word = dfmt.trim(word)
                    end
                    while word:len() >= width do
                        local fragment = word:sub(1, width)
                        word = word:sub(width+1)
                        table.insert(new_lines, 1, fragment)
                    end
                    if word:len() > 0 then
                        table.insert(chunks, word)
                        idx = word:len()
                    end
                    holdover_word = false
                else
                    if word:len() + idx < width then
                        table.insert(chunks, word)
                        idx = idx + word:len()
                        holdover_word = false
                    else
                        table.insert(new_lines, 1, table.concat(chunks, ''))
                        chunks = {}
                        idx = 0
                    end
                end
            end
        end
         
         if #chunks > 0 then
            table.insert(new_lines, 1, table.concat(chunks, ''))
        end
        
        t.width = width
        t.render = new_lines
        
        if DEBUG then for _, line in ipairs(t.render) do dbglog('    %s', line) end end
    end
end

local function update_roster(list, w)
    w.roster:clear()
    local y = 1
    for i, name in ipairs(list) do
        if y < w.ysize - 2 then
            w.roster:mvaddstr(y, 1, name)
            y = y + 1
        end
    end
    w.roster:border()
    w.roster:refresh()
end

local function paint_status(sw)
    dbglog('paint_status() called')
    local status_text = string.format('%s @ %s', uname, sock:getsockname())
    sw:move(0, 0)
    sw:addch(curses.ACS_LTEE)
    sw:addch(SPACE)
    sw:addstr(status_text)
    sw:addch(SPACE)
    local _, winwid = sw:getmaxyx()
    local _, x = sw:getyx()
    sw:hline(curses.ACS_HLINE, winwid-x)
    sw:refresh()
end

local function handle_chunk(msg, w)
    
    -- The only protocol message that doesn't decode as a table.
    if msg == 'Ping' then
        enqueue('Ping')
        return false, nil
    end

    if msg['Text'] then
        local t = msg['Text']
        for _, line in ipairs(t.lines) do
            add_line(string.format('%s: %s', t.who, line))
        end
    
    elseif msg['Join'] then
        local t = msg['Join']
        add_line(string.format('* %s joins %s.', t.who, t.what))
        -- Automatically request updated roster on each join.
        local u = { ['Query'] = { ['what'] = 'roster', ['arg'] = '_', }, }
        enqueue(u)
    
    elseif msg['Name'] then
        local t = msg['Name']
        add_line(string.format('* "%s" is now known as "%s".', t.who, t.new))
        if t.who == uname then
            uname = t.new
            paint_status(w.status)
        end
        -- Automatically request updated roster on each name change.
        local u = { ['Query'] = { ['what'] = 'roster', ['arg'] = '_', }, }
        enqueue(u)

    elseif msg['Leave'] then
        local t = msg['Leave']
        add_line(string.format('* %s leaves: %s', t.who, t.message))
        -- Automatically request updated roster on each leave.
        local u = { ['Query'] = { ['what'] = 'roster', ['arg'] = '_', }, }
        enqueue(u)
    
    elseif msg['List'] then
        local t = msg['List']
        if t['what'] == 'roster' then
            update_roster(t['items'], w)
        else
            add_line(string.format('* List: %s', t.what))
            add_line(table.concat(t.items, ', '))
        end
    
    elseif msg['Info'] then
        add_line(string.format('* %s', msg['Info']))
    
    elseif msg['Err'] then
        add_line(string.format('# ERROR: %s', msg['Err']))
    
    elseif msg['Logout'] then
        add_line("You have been logged out.")
        return false, msg['Logout']
    
    end
    
    return true, nil
end

local function resize(w)
    w.ysize = curses.lines()
    w.xsize = curses.cols()
    local real_roster_width = ROSTER_WIDTH + 2
    local main_h = w.ysize - 2
    local main_w = w.xsize - real_roster_width
    w.main:resize(main_h, main_w)
    w.roster:move_window(0, main_w)
    w.roster:resize(main_h+1, real_roster_width)
    w.roster:border()
    w.status:move_window(main_h, 0)
    w.status:resize(1, main_w)
    w.input:move_window(main_h+1, 0)
    w.input:resize(1, w.xsize)
    
    if DEBUG then
        dbglog('resize()')
        dbglog('    main:')
        window_debug(w.main)
        dbglog('    roster:')
        window_debug(w.roster)
        dbglog('    input:')
        window_debug(w.input)
    end
end

local function paint_lines(mw)
    mw:erase()
    local nlines, ncols = mw:getmaxyx()
    local y = nlines - 1
    local n = 1
    while (y > 0) and lines[n] do
        line_t = lines[n]
        if line_t.width ~= ncols then wrap(line_t, ncols) end
        for _, line in ipairs(line_t.render) do
            if y > 0 then
                mw:mvaddstr(y, 0, line)
                y = y - 1
            end
        end
        n = n + 1
    end
    mw:refresh()
end

local input = {
    ['chars'] = {},
    ['ip'] = 1,
}

local function paint_input(iw)
    dbglog('paint_input() called')
    if DEBUG then window_debug(iw) end
    iw:move(0, 0)
    for x, ch in ipairs(input.chars) do
        if x == input.ip then
            iw:attron(curses.A_REVERSE)
            iw:addch(ch)
            iw:attroff(curses.A_REVERSE)
        else
            iw:addch(ch)
        end
    end
    if input.ip > #input.chars then
        iw:attron(curses.A_REVERSE)
        iw:addch(SPACE)
        iw:attroff(curses.A_REVERSE)
    end
    iw:clrtoeol()
    iw:refresh()
    dbglog('paint_input(): screen refreshed')
end

local function get_input(w)
    local get_again = true
    while get_again do
        local ch, err = w.s:getch()
        if ch then
            dbglog("get_input(): got keycode %d", ch)
            if ch == NEWLINE then
                local t = {}
                for _, n in ipairs(input.chars) do table.insert(t, string.char(n)) end
                input.chars = {}
                input.ip = 1
                paint_input(w.input)
                return table.concat(t, '')
            elseif ch == curses.KEY_BACKSPACE then
                if input.ip > 1 then
                    input.ip = input.ip - 1
                    table.remove(input.chars, input.ip)
                end
            elseif ch == curses.KEY_LEFT then
                if input.ip > 1 then input.ip = input.ip - 1 end
            elseif ch == curses.KEY_RIGHT then
                if input.ip < ( #input.chars + 1 ) then input.ip = input.ip + 1 end
            elseif ch < 127 then
                table.insert(input.chars, input.ip, ch)
                input.ip = input.ip + 1
                if DEBUG then
                    local t = {}
                    for _, n in ipairs(input.chars) do table.insert(t, string.char(n)) end
                    dbglog('input buffer: "%s", ip at %d', table.concat(t, ''), input.ip)
                end
            end
            paint_input(w.input)
        else
            get_again = false
        end
    end
    
    return nil
end

local function handle_user_input(line, w)
    local t = nil
    local cmd, rest = line:match('^%s*(;%S+)%s*(.-)$')
    if cmd then
        cmd = cmd:lower()
        if cmd == ';quit' then
            t = { ['Logout'] = rest, }
        elseif cmd == ';name' then
            t = { ['Name'] = { ['new'] = rest, ['who'] = '_', }, }
        elseif cmd == ';join' then
            t = { ['Join'] = { ['what'] = rest, ['who'] = '_', }, }
        elseif cmd == ';who' then
            t = { ['Query'] = { ['what'] = 'who', arg = rest, },  }
        else
            add_line(string.format('# Error: Unrecognized command: %s', cmd))
            paint_lines(w.main)
        end
    else
        t = { ['Text'] = { ['who'] = '_', ['lines'] = { line }, }, }
    end
    if t then
        enqueue(t)
    end
end

local function main()
    local stdscr = curses.initscr()
    curses.echo(false)
    curses.nl(false)
    curses.cbreak(true)
    curses.curs_set(0)
    stdscr:keypad(true)
    stdscr:timeout(TICK_TIMEOUT)
    stdscr:clear()
    
    local w = {}
    w.s = stdscr
    w.ysize  = curses.lines()
    w.xsize  = curses.cols()
    w.main   = curses.newwin(5, 5, 0, 0)
    w.roster = curses.newwin(5, ROSTER_WIDTH, 0, 5)
    w.input  = curses.newwin(1, w.xsize, w.ysize-1, 0)
    w.status = curses.newwin(1, w.xsize, w.ysize-2, 0)
    
    resize(w)
    
    --paint_status(w.status)
    
    local run = true
    local end_message = ''
    while run do
    
        local user_input = get_input(w)
        if user_input then
            handle_user_input(user_input, w)
        end
        local err = nudge(sock)
        if err then panick(err) end
    
        local input_chunks, err = try_read(sock)
        local should_redraw = false
        if input_chunks then
            for _, chunk in ipairs(input_chunks) do
                local redraw, leave_msg = handle_chunk(chunk, w)
                if leave_msg then
                    run = false
                    end_message = leave_msg .. '\n'
                end
                if redraw then should_redraw = true end
            end
            if should_redraw then
                paint_status(w.status)
                paint_lines(w.main)
                paint_input(w.input)
            end
        end
        
        if err then
            run = false
            end_message = end_message .. '\n'
        end
        
    end
    
    curses.endwin()
    if end_message then print(end_message) end
    os.exit(0)
    
end

uname = argz[1]
if not uname then
    print("First argument must be a user name.");
    os.exit(2)
end

-- truncating log file
if DEBUG then
    local f = io.open(LOG_FILE, 'w')
    f:close()
end

local join_obj = { ['Name'] = { ['who'] = '_', ['new']  = uname, }, }
--local join_bytes = json.encode(join_obj)

sock, err = socket.connect(ADDR, PORT)
print(err)
if not sock:setoption('tcp-nodelay', true) then
    print('Unable to set option "tcp-nodelay" on socket.')
    os.exit(1)
end
if not sock:settimeout(0, 'b') then
    print("Unable to set timeout on socket.")
    os.exit(1)
end
local err = blocking_send(sock, join_obj)
if err then
    print(string.format("Error sending join name change: %s", err))
    os.exit(1)
end

xpcall(main, panick)


