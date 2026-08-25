-- Background launcher provider for Shell Command.
--
-- Type `/cmd <command>` to run a command in the background (fire-and-forget):
-- no terminal window, no captured output, no exit-code feedback. Processes
-- are detached (double-fork) and never time-limited.
local core = require("./shell_core.luau")
core.mode = "background"
onQuery = function(query) core.onQuery(query) end
onActivate = function(id) core.onActivate(id) end
