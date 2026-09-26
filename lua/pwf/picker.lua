--- fzf-lua pickers over pwf tasks.
local tasks = require("pwf.tasks")

local M = {}

--- Matches `TaskListLimit::MAX` in `pwf-models`.
local TASK_LIMIT_MAX = 100000

local function notify_error(err)
  vim.notify("pwf: " .. err, vim.log.levels.ERROR)
end

local function fzf_lua()
  local loaded, fzf = pcall(require, "fzf-lua")
  if not loaded then
    notify_error("task pickers require fzf-lua (https://github.com/ibhagwan/fzf-lua)")
    return nil
  end
  return fzf
end

local previewer_class
--- Previews the task file behind an entry with fzf-lua's file previewer.
local function task_previewer()
  if previewer_class then
    return previewer_class
  end
  local file_previewer = require("fzf-lua.previewer.builtin").buffer_or_file
  previewer_class = file_previewer:extend()
  -- fzf-lua discards the result when the cursor has moved to another entry.
  function previewer_class:parse_entry(entry, callback)
    local id = tasks.entry_id(entry)
    tasks.file(id, function(err, file)
      if err then
        callback({ title = " " .. id .. " ", content = { { { "pwf: " .. err, "ErrorMsg" } } } })
      else
        callback(file_previewer.parse_entry(self, file.path))
      end
    end)
  end
  return previewer_class
end

--- @class pwf.PickerState
--- @field scope "project"|"global"
--- @field status "active"|"all"
--- @field limit integer
--- @field context_paths string[]
--- @field query string?

--- @param state pwf.PickerState
--- @param title string
--- @param select_commands table<string, string> fzf key to the command passed to `on_select`
--- @param on_select fun(id: string, command: string)
local function open(state, title, select_commands, on_select)
  local query = {
    context_paths = state.scope == "project" and state.context_paths or nil,
    status = state.status,
    limit = state.limit,
  }
  tasks.list(query, function(err, listing)
    if err then
      return notify_error(err)
    end
    local fzf = fzf_lua()
    if not fzf then
      return
    end
    local function reopen(changes)
      return function(_, opts)
        local next_state = vim.tbl_extend("force", state, changes, { query = opts.last_query })
        open(next_state, title, select_commands, on_select)
      end
    end
    local actions = {
      ["ctrl-g"] = {
        fn = reopen({ scope = state.scope == "project" and "global" or "project" }),
        header = state.scope == "project" and "search all projects" or "search the current project",
      },
      ["alt-s"] = {
        fn = reopen({ status = state.status == "active" and "all" or "active" }),
        header = state.status == "active" and "include closed tasks" or "show active tasks",
      },
    }
    if listing.hidden > 0 and state.limit < TASK_LIMIT_MAX then
      actions["alt-m"] = {
        fn = reopen({ limit = math.min(state.limit * 2, TASK_LIMIT_MAX) }),
        header = ("load more (%d hidden)"):format(listing.hidden),
      }
    end
    for key, command in pairs(select_commands) do
      actions[key] = {
        fn = function(selected)
          if selected[1] then
            on_select(tasks.entry_id(selected[1]), command)
          end
        end,
        header = false,
      }
    end
    local scope = listing.project or "all projects"
    fzf.fzf_exec(listing.entries, {
      prompt = "Tasks> ",
      query = state.query,
      winopts = { title = (" %s · %s · %s "):format(title, scope, state.status) },
      fzf_opts = { ["--no-multi"] = true },
      -- fzf-lua copies previewer tables, so it instantiates classes only through `_ctor`.
      previewer = { _ctor = task_previewer },
      actions = actions,
    })
  end)
end

local function initial_state()
  local config = require("pwf").config
  return {
    scope = config.task_scope,
    status = config.task_status,
    limit = config.task_limit,
    context_paths = tasks.context_paths(),
  }
end

--- Opens the selected task's Markdown file.
function M.tasks()
  local commands = { enter = "edit", ["ctrl-s"] = "split", ["ctrl-v"] = "vsplit", ["ctrl-t"] = "tabedit" }
  open(initial_state(), "pwf tasks", commands, function(id, command)
    tasks.open(id, command, function(err)
      if err then
        notify_error(err)
      end
    end)
  end)
end

--- Inserts `[[ID]]` for the selected task at the cursor captured in `target`.
--- @param target pwf.ReferenceTarget
function M.reference(target)
  local reference = require("pwf.reference")
  open(initial_state(), "pwf reference", { enter = "insert" }, function(id)
    local _, err = reference.insert(id, target)
    if err then
      notify_error(err)
    end
  end)
end

return M
