-- Terminal launcher provider for Shell Command.
--
-- Type `/sh <command>` to open a real terminal running the command (upstream
-- behaviour): live output, TUI apps, native history, and a `cd` folder-browser
-- mode to open a terminal inside a chosen directory.
local core = require("./shell_core.luau")
core.mode = "terminal"
onQuery = function(query) core.onQuery(query) end
onActivate = function(id) core.onActivate(id) end
