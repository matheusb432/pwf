--- Task operations behind the pickers.
local client = require("pwf.client")

local M = {}

--- Returns the paths that identify the current project: the current file, then the working
--- directory.
--- @return string[]
function M.context_paths()
  local paths = {}
  local name = vim.api.nvim_buf_get_name(0)
  if name ~= "" and vim.bo.buftype == "" then
    table.insert(paths, name)
  end
  table.insert(paths, vim.fn.getcwd())
  return paths
end

--- @class pwf.TaskQuery
--- @field context_paths string[]? project-inference paths; omission lists every project
--- @field status "active"|"all"
--- @field limit integer tasks listed before the rest count as hidden

--- @class pwf.TaskListing
--- @field project string? the inferred project, when one owns a context path
--- @field entries string[] one line per task, starting with its ID
--- @field hidden integer matching tasks beyond the limit

--- Lists tasks as picker entries.
--- @param query pwf.TaskQuery
--- @param callback fun(err: string?, listing: pwf.TaskListing?)
function M.list(query, callback)
  client.request("list_tasks", {
    context_paths = query.context_paths,
    status = query.status,
    limit = query.limit,
  }, callback)
end

--- Returns the task ID that starts a picker entry.
--- @param entry string
--- @return string
function M.entry_id(entry)
  return entry:match("^(%S+)")
end

--- Resolves the Markdown file that stores a task.
--- @param id string
--- @param callback fun(err: string?, file: { path: string }?)
function M.file(id, callback)
  client.request("task_file", { id = id }, callback)
end

--- Opens a task's Markdown file in the current window.
--- @param id string
--- @param command "edit"|"split"|"vsplit"|"tabedit"
--- @param callback fun(err: string?, file: { path: string }?)
function M.open(id, command, callback)
  M.file(id, function(err, file)
    if err then
      return callback(err)
    end
    local opened, open_err = pcall(vim.cmd[command], vim.fn.fnameescape(file.path))
    if not opened then
      return callback(tostring(open_err))
    end
    callback(nil, file)
  end)
end

return M
