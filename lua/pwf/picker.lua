local M = {}
local TASK_LIMIT_MAX = 100000

local function notify_error(err)
  vim.notify("pwf: " .. err, vim.log.levels.ERROR)
end

local function task_id_search(items)
  local query = items[1] or ""
  local project, number = vim.trim(query):match("^([A-Za-z]+)%-?(%d+)$")
  if project and #project >= 2 and #project <= 4 and #number <= 4 then
    return ("^%s-%04d$ | %s"):format(project:upper(), tonumber(number), query)
  end
  return query
end

local function previewer(resolve)
  local base = require("fzf-lua.previewer.builtin").buffer_or_file
  local class = base:extend()
  function class:parse_entry(entry)
    local match = resolve(entry)
    local file = base.parse_entry(self, match and match.path or "")
    if match then file.line, file.col = match.line, match.col end
    return file
  end
  return class
end

local function open(fzf, state, listing)
  local records = require("pwf.records")
  if not listing then
    records.list({
      context_paths = state.scope == "project" and state.context_paths or nil,
      status = state.status,
      limit = state.limit,
    }, function(err, loaded_listing)
      if err then return notify_error(err) end
      open(fzf, state, loaded_listing)
    end)
    return
  end
  local function resolve(entry)
    if listing then return records.resolve(listing, entry) end
  end
  local function reopen(changes, opts, retain_listing)
    open(fzf, vim.tbl_extend("force", state, changes, { query = opts.last_query }), retain_listing and listing or nil)
  end
  local actions = {
    ["ctrl-g"] = {
      fn = function(_, opts)
        reopen({ mode = state.mode == "names" and "contents" or "names" }, opts, true)
      end,
      header = state.mode == "names" and "search contents" or "search task IDs",
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
          prompt = "Task status:",
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
        local file = selected[1] and resolve(selected[1])
        if not file then return end
        local open_err = records.open(file, command)
        if open_err then notify_error(open_err) end
      end,
      header = false,
    }
  end
  local content_mode = state.mode == "contents"
  fzf.fzf_exec(function(_, write_lines)
    if listing then
      write_lines(content_mode and listing.contents or vim.tbl_filter(function(entry)
        return entry:match("^[A-Z]+%-%d+\t") ~= nil
      end, listing.names))
    end
    write_lines(nil)
  end, {
    prompt = content_mode and "Content> " or "Task ID> ",
    query = state.query,
    no_resume = true,
    no_hide = true,
    -- Keep action queries without retaining the listing in fzf-lua's resume options.
    __resume_set = function(what, value, opts)
      if what == "query" then opts.last_query = value end
    end,
    winopts = {
      title = (" pwf · %s · %s tasks%s · %s "):format(listing.project or "all projects", state.status,
        content_mode and " + notes" or "", content_mode and "contents" or "IDs"),
      -- Actions run after the window closes; release this view after they finish.
      on_close = function() vim.schedule(function() listing = nil end) end,
    },
    fzf_opts = {
      ["--no-multi"] = true,
      ["--delimiter"] = "[\t ]",
      ["--with-nth"] = "3..",
      -- fzf applies --nth after --with-nth; the first visible field is the ID.
      ["--nth"] = not content_mode and "1" or false,
      ["--ignore-case"] = not content_mode,
      ["--extended"] = true,
      ["--tiebreak"] = "index",
    },
    keymap = not content_mode and {
      fzf = { ["start,change"] = "transform-search:" .. fzf.shell.stringify_data(task_id_search, {}, "{q}") },
    } or nil,
    -- fzf-lua deep-copies previewers; construct the class for this picker instance.
    previewer = { _ctor = function() return previewer(resolve) end },
    actions = actions,
  })
end

function M.open()
  local loaded, fzf = pcall(require, "fzf-lua")
  if not loaded then return notify_error("the picker requires fzf-lua") end
  local records = require("pwf.records")
  local config = require("pwf").config
  open(fzf, {
    mode = "names",
    scope = config.task_scope,
    status = config.task_status,
    limit = config.task_limit,
    context_paths = records.context_paths(),
  })
end

return M
