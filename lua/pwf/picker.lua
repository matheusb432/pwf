local records = require("pwf.records")
local M = {}
local TASK_LIMIT_MAX = 100000

local function notify_error(err)
  vim.notify("pwf: " .. err, vim.log.levels.ERROR)
end

local function previewer(listing)
  local base = require("fzf-lua.previewer.builtin").buffer_or_file
  local class = base:extend()
  function class:parse_entry(entry)
    local match = records.resolve(listing, entry)
    local file = base.parse_entry(self, match.path)
    file.line, file.col = match.line, match.col
    return file
  end
  return class
end

local function open(state, listing)
  if not listing then
    records.list({
      context_paths = state.scope == "project" and state.context_paths or nil,
      status = state.status,
      limit = state.limit,
    }, function(err, loaded_listing)
      if err then return notify_error(err) end
      open(state, loaded_listing)
    end)
    return
  end
  local loaded, fzf = pcall(require, "fzf-lua")
  if not loaded then return notify_error("the picker requires fzf-lua") end
  local function reopen(changes, opts, retain_listing)
    open(vim.tbl_extend("force", state, changes, { query = opts.last_query }), retain_listing and listing or nil)
  end
  local actions = {
    ["ctrl-g"] = {
      fn = function(_, opts)
        reopen({ mode = state.mode == "names" and "contents" or "names" }, opts, true)
      end,
      header = state.mode == "names" and "search contents" or "search names",
    },
    ["alt-g"] = {
      fn = function(_, opts)
        reopen({ scope = state.scope == "project" and "global" or "project" }, opts)
      end,
      header = state.scope == "project" and "all projects" or "current project",
    },
    ["alt-s"] = {
      fn = function(_, opts)
        vim.ui.select({ "active", "backlog", "done", "cancelled", "all" }, {
          prompt = "Task status (notes always included):",
        }, function(status) reopen({ status = status or state.status }, opts) end)
      end,
      header = "task status",
    },
  }
  if listing.hidden > 0 and state.limit < TASK_LIMIT_MAX then
    actions["alt-m"] = {
      fn = function(_, opts) reopen({ limit = math.min(state.limit * 2, TASK_LIMIT_MAX) }, opts) end,
      header = ("load more (%d hidden)"):format(listing.hidden),
    }
  end
  for key, command in pairs({ enter = "edit", ["ctrl-s"] = "split", ["ctrl-v"] = "vsplit", ["ctrl-t"] = "tabedit" }) do
    actions[key] = {
      fn = function(selected)
        local file = selected[1] and records.resolve(listing, selected[1])
        if not file then return end
        local open_err = records.open(file, command)
        if open_err then notify_error(open_err) end
      end,
      header = false,
    }
  end
  local content_mode = state.mode == "contents"
  fzf.fzf_exec(content_mode and listing.contents or listing.names, {
    prompt = content_mode and "Content> " or "Names> ",
    query = state.query,
    winopts = {
      title = (" pwf · %s · %s tasks + notes · %s "):format(listing.project or "all projects", state.status,
        content_mode and "contents" or "names"),
    },
    fzf_opts = { ["--no-multi"] = true, ["--delimiter"] = "\t", ["--with-nth"] = "3..", ["--tiebreak"] = "index" },
    -- fzf-lua deep-copies previewers; construct the class for this picker instance.
    previewer = { _ctor = function() return previewer(listing) end },
    actions = actions,
  })
end

function M.open()
  local config = require("pwf").config
  open({
    mode = "names",
    scope = config.task_scope,
    status = config.task_status,
    limit = config.task_limit,
    context_paths = records.context_paths(),
  })
end

return M
