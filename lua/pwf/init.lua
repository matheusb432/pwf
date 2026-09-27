--- Neovim frontend for pwf tasks and notes.
local M = {}

--- @class pwf.Config
--- @field cmd string[] starts the pwf-nvim child process
--- @field task_limit integer records a picker lists before offering to load more
--- @field task_scope "project"|"global" initial project scope
--- @field task_status "active"|"backlog"|"done"|"cancelled"|"all" initial task status filter; notes remain visible

--- @type pwf.Config
local defaults = {
  cmd = { "pwf-nvim" },
  task_limit = 5000,
  task_scope = "project",
  task_status = "active",
}

--- @type pwf.Config
M.config = vim.deepcopy(defaults)

--- Replaces the configuration; omitted fields keep their defaults. Setup is optional.
--- @param opts pwf.Config?
function M.setup(opts)
  local config = vim.tbl_extend("force", vim.deepcopy(defaults), opts or {})
  vim.validate("cmd", config.cmd, function(cmd)
    return vim.islist(cmd) and #cmd > 0 and vim.iter(cmd):all(function(part)
      return type(part) == "string"
    end)
  end, "a non-empty list of strings")
  vim.validate("task_limit", config.task_limit, function(limit)
    return type(limit) == "number" and limit % 1 == 0 and limit >= 1 and limit <= 100000
  end, "an integer from 1 to 100000")
  vim.validate("task_scope", config.task_scope, function(scope)
    return scope == "project" or scope == "global"
  end, '"project" or "global"')
  vim.validate("task_status", config.task_status, function(status)
    return vim.tbl_contains({ "active", "backlog", "done", "cancelled", "all" }, status)
  end, '"active", "backlog", "done", "cancelled", or "all"')
  M.config = config
end

--- Searches task and note names, with a toggle for their saved contents.
function M.tasks()
  require("pwf.picker").open()
end

M.notes = M.tasks

return M
