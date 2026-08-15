-- Shell Command launcher provider.
--
-- Type `/sh <command>` in the launcher to run a shell command in the
-- background — no terminal window is opened. Fish-style autosuggestions: as
-- you type it queries the shell's own completion engine (`fish -c 'complete
-- -C "<query>"'`, falling back to bash `compgen -c`) so suggestions grow
-- dynamically with your system — no hardcoded list. Commands from your own
-- history and user-defined snippets are offered too. Selecting an entry
-- launches the command detached via `noctalia.runAsync` (fire-and-forget):
-- no output is captured and there is no exit-code feedback, but commands are
-- never time-limited and keep running after Noctalia exits.

local HISTORY_FILE = "history"
local HISTORY_LIMIT = 100
local MAX_SUGGESTIONS = 8

local history = {}

local function shellQuote(s)
	return "'" .. s:gsub("'", "'\"'\"'") .. "'"
end

local function trim(s)
	return noctalia.string.trim(tostring(s or ""))
end

-- Private per-plugin storage (XDG state); the host creates the directory on
-- every call. nil (with a log line) when no state directory resolves.
local function dataDir()
	local dir, err = noctalia.pluginDataDir()
	if dir == nil then
		noctalia.log("shell-command: pluginDataDir failed: " .. tostring(err))
		return nil
	end
	return dir
end

local function loadHistory()
	local dir = dataDir()
	if dir == nil then
		return
	end
	local raw = noctalia.readFile(dir .. "/" .. HISTORY_FILE)
	if type(raw) ~= "string" or raw == "" then
		return
	end
	history = {}
	for line in raw:gmatch("[^\r\n]+") do
		line = trim(line)
		if line ~= "" then
			history[#history + 1] = line
		end
	end
end

local function saveHistory()
	local dir = dataDir()
	if dir == nil then
		return
	end
	local ok, err = noctalia.writeFile(dir .. "/" .. HISTORY_FILE, table.concat(history, "\n"))
	if not ok then
		noctalia.log("shell-command: could not persist history: " .. tostring(err))
	end
end

-- Record a run command: move it to the front (most recent), dedupe, cap size.
local function recordHistory(cmd)
	for i, entry in ipairs(history) do
		if entry == cmd then
			table.remove(history, i)
			break
		end
	end
	table.insert(history, 1, cmd)
	if #history > HISTORY_LIMIT then
		for _ = HISTORY_LIMIT + 1, #history do
			table.remove(history)
		end
	end
	saveHistory()
end

-- Wrap a command in the background wrapper and run it via the user's own
-- interactive shell (`$SHELL -ic`) so aliases and functions from the
-- login/rc config are available — a bare `-c` ignores them and breaks things
-- like fish aliases. No terminal is involved: noctalia.runAsync detaches the
-- process (double-fork) and never time-limits it. An interactive `-i` shell
-- without a tty may print a harmless job-control warning to stderr, which is
-- discarded together with all other output.
local function buildBackground(inner)
	local shell = noctalia.getenv("SHELL") or "sh"
	return shell .. " -ic " .. shellQuote(inner)
end

-- Build the command actually launched.
--
-- If the query itself is a `cd <dir> && <cmd>` compound, we cd into `<dir>`
-- instead of the workspace — that way navigation and a command can be combined
-- in one launch (`/sh cd ~/proj && ls`). Otherwise, if a default workspace is
-- configured, cd there first.
local function buildCommand(cmd)
	local inner = cmd
	-- Try to split a leading `cd <path> && <rest>` compound, honouring quoted
	-- paths, spaces and `\ ` escapes inside the directory argument.
	local dir = cmd:match("^%s*cd%s+(.+)$")
	if dir then
		local i, n = 1, #dir
		local out = {}
		local quote
		local done
		while i <= n do
			local c = dir:sub(i, i)
			if quote then
				if c == quote then
					quote = nil
				elseif c == "\\" and i < n then
					out[#out + 1] = dir:sub(i + 1, i + 1)
					i = i + 1
				else
					out[#out + 1] = c
				end
			elseif c == '"' or c == "'" then
				quote = c
			elseif c == " " then
				done = true
				break
			else
				out[#out + 1] = c
			end
			i = i + 1
		end
		if not done then
			i = n + 1 -- path ran to end; nothing appended
		end
		local rest = dir:sub(i):match("^%s*&&%s*(.+)$")
		if #out > 0 and rest then
			local path = table.concat(out)
			local abs = path:gsub("^~/+", (noctalia.getenv("HOME") or "/") .. "/")
			inner = "cd " .. shellQuote(abs) .. " && " .. rest
		else
			inner = cmd
		end
	else
		local cwd = noctalia.getConfig("default_workspace")
		if type(cwd) == "string" and cwd ~= "" then
			inner = "cd " .. shellQuote(cwd) .. " && " .. cmd
		end
	end
	return buildBackground(inner)
end

-- The first token of the command, used to detect whether the binary exists.
local function firstToken(cmd)
	local token = cmd:match("^%s*([^%s]+)")
	return token or ""
end

local function runEntry(cmd, subtitle)
	return {
		id = "run:" .. cmd,
		title = noctalia.tr("run_title", { command = cmd }),
		subtitle = subtitle,
		glyph = "terminal",
	}
end

-- Rebuild the full command from a completion token. The completion engine
-- returns only the token being completed, so we re-attach the typed prefix.
local function fullCommand(query, comp)
	local prefix = query:match("^(.*%s)") or ""
	return prefix .. comp
end

-- Parse `fish -c 'complete -C "<query>"'` output: one completion per line,
-- optionally `completion<TAB>description`. We keep the completion token.
local function parseFish(output)
	local comps = {}
	for line in (tostring(output or "") .. "\n"):gmatch("(.-)\n") do
		line = trim(line)
		if line ~= "" then
			local comp = line:match("^([^\t]+)")
			if comp and comp ~= "" then
				comps[#comps + 1] = comp
			end
		end
	end
	return comps
end

-- Parse `compgen -c` output: one command name per line.
local function parseCompgen(output)
	local comps = {}
	for line in (tostring(output or "") .. "\n"):gmatch("(.-)\n") do
		line = trim(line)
		if line ~= "" then
			comps[#comps + 1] = line
		end
	end
	return comps
end

-- Query the shell's completion engine for the typed prefix. Prefers fish
-- (full subcommand/flag completions); falls back to bash `compgen -c` for
-- command names when fish is unavailable.
local function fetchCompletions(query, callback)
	if noctalia.commandExists("fish") then
		noctalia.runAsync("fish -c " .. shellQuote("complete -C " .. shellQuote(query)), function(result)
			callback(parseFish(result.stdout))
		end)
		return
	end
	if noctalia.commandExists("bash") then
		local word = query:match("([^%s]+)%s*$") or ""
		noctalia.runAsync("bash -c " .. shellQuote("compgen -c " .. shellQuote(word)), function(result)
			callback(parseCompgen(result.stdout))
		end)
		return
	end
	callback({})
end

local pendingQuery = nil

function onQuery(query)
	query = trim(query)
	pendingQuery = query

	if query == "" then
		-- Empty query: hint, then user snippets, then recent commands.
		local results = {
			{
				id = "",
				title = noctalia.tr("hint_title"),
				subtitle = noctalia.tr("hint_subtitle"),
				glyph = "terminal",
			},
		}
		local snippets = noctalia.getConfig("snippets")
		if type(snippets) == "table" then
			for _, snippet in ipairs(snippets) do
				snippet = trim(snippet)
				if snippet ~= "" then
					results[#results + 1] = {
						id = "fill:" .. snippet,
						title = snippet,
						subtitle = noctalia.tr("snippet_subtitle"),
						glyph = "bookmark",
					}
				end
			end
		end
		for i = 1, math.min(MAX_SUGGESTIONS, #history) do
			results[#results + 1] = {
				id = "fill:" .. history[i],
				title = history[i],
				subtitle = noctalia.tr("history_subtitle"),
				glyph = "history",
			}
		end
		launcher.setResults(query, results)
		return
	end

	local token = firstToken(query)
	local subtitle = noctalia.tr("run_subtitle")
	if token ~= "" and noctalia.commandExists(token) then
		subtitle = noctalia.tr("run_subtitle_found", { command = token })
	end

	-- Show the exact command immediately, then fill in suggestions async.
	launcher.setResults(query, { runEntry(query, subtitle) })

	fetchCompletions(query, function(comps)
		if pendingQuery ~= query then
			return -- a newer query superseded this one
		end
		local results = { runEntry(query, subtitle) }
		-- Snap-complete: a single distinct expanding candidate (a completion that
		-- actually extends the typed text) becomes the one Enter action — run the
		-- completed command directly without first picking a fill row. Multiple
		-- candidates: keep normal suggestions. Uses its own seen-table so the
		-- suggestion pass below still emits the full list.
		local seenScan, single, count = { [query] = true }, nil, 0
		for _, comp in ipairs(comps) do
			local full = fullCommand(query, comp)
			if full ~= query and not seenScan[full] then
				seenScan[full] = true
				single, count = full, count + 1
			end
		end
		if count == 1 and single then
			results = { runEntry(single, noctalia.tr("snap_subtitle", { command = single })) }
		end
		local seen = { [query] = true }
		for _, comp in ipairs(comps) do
			local full = fullCommand(query, comp)
			if not seen[full] then
				seen[full] = true
				results[#results + 1] = {
					id = "fill:" .. full,
					title = full,
					subtitle = noctalia.tr("suggest_subtitle"),
					glyph = "lightbulb",
				}
			end
			if #results >= 1 + MAX_SUGGESTIONS then
				break
			end
		end
		-- History matches, ranked after shell completions.
		for _, cmd in ipairs(history) do
			if cmd:sub(1, #query) == query and not seen[cmd] then
				seen[cmd] = true
				results[#results + 1] = {
					id = "fill:" .. cmd,
					title = cmd,
					subtitle = noctalia.tr("history_subtitle"),
					glyph = "history",
				}
			end
			if #results >= 1 + MAX_SUGGESTIONS then
				break
			end
		end
		launcher.setResults(query, results)
	end)
end

function onActivate(id)
	if id == "" then
		return
	end

	local kind, value = id:match("^(%a+):(.+)$")
	if not kind or not value then
		noctalia.log("shell-command: onActivate received malformed id: " .. tostring(id))
		return
	end

	if kind == "fill" then
		launcher.setQuery(value)
	elseif kind == "run" then
		local ok = noctalia.runAsync(buildCommand(value))
		if ok then
			recordHistory(value)
		else
			-- Launch rejected (e.g. global async command limit reached).
			noctalia.notifyError(noctalia.tr("run_start_failed"), value)
		end
	else
		noctalia.log("shell-command: unknown activation kind: " .. kind)
	end
end

loadHistory()
