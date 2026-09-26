--- Runs the pwf-nvim child process and correlates its asynchronous replies.
---
--- Requests travel as `request` notifications so the editor never blocks on pwf-server. The child
--- answers each one by calling `resolve` through `nvim_exec_lua`.
local M = {}

--- Must match `PROTOCOL_VERSION` in `crates/pwf-nvim/src/lib.rs`.
local PROTOCOL_VERSION = 1
--- Exceeds the child's 10-second request timeout, so the child's own error normally arrives first.
local REQUEST_TIMEOUT_MS = 15000
local STDERR_LINES_MAX = 20

local job_id = nil ---@type integer?
local pending = {} ---@type table<integer, fun(err: string?, value: any)>
local next_request_id = 0
local stderr_lines = {} ---@type string[]

--- Delivers one outcome outside the RPC call that produced it; later outcomes are ignored.
local function settle(id, err, value)
  local callback = pending[id]
  if not callback then
    return
  end
  pending[id] = nil
  vim.schedule(function()
    callback(err, value)
  end)
end

local function start()
  local command = vim.deepcopy(require("pwf").config.cmd)
  vim.list_extend(command, { "--protocol", tostring(PROTOCOL_VERSION) })
  if vim.fn.executable(command[1]) ~= 1 then
    return nil,
      ("%s is not executable; install pwf-nvim with `cargo install pwf-app --locked --features nvim`"):format(
        command[1]
      )
  end
  stderr_lines = {}
  local started, id = pcall(vim.fn.jobstart, command, {
    rpc = true,
    on_stderr = function(_, data)
      for _, line in ipairs(data) do
        if line ~= "" then
          table.insert(stderr_lines, line)
          if #stderr_lines > STDERR_LINES_MAX then
            table.remove(stderr_lines, 1)
          end
        end
      end
    end,
    on_exit = function(exited_id, code)
      if job_id == exited_id then
        job_id = nil
      end
      local detail = #stderr_lines > 0 and (":\n" .. table.concat(stderr_lines, "\n")) or ""
      for request_id in pairs(pending) do
        settle(request_id, ("pwf-nvim exited with status %d%s"):format(code, detail))
      end
    end,
  })
  if not started or id <= 0 then
    return nil, ("could not start %s: %s"):format(command[1], tostring(id))
  end
  job_id = id
  return id
end

--- Sends one operation to the child, starting it on first use.
--- @param operation string
--- @param params table
--- @param callback fun(err: string?, value: any) called once, on the main loop
function M.request(operation, params, callback)
  if not job_id then
    local _, err = start()
    if err then
      vim.schedule(function()
        callback(err)
      end)
      return
    end
  end
  next_request_id = next_request_id + 1
  local id = next_request_id
  pending[id] = callback
  local sent, err = pcall(vim.rpcnotify, job_id, "request", id, operation, params)
  if not sent then
    settle(id, "could not reach pwf-nvim: " .. tostring(err))
    return
  end
  vim.defer_fn(function()
    settle(id, ("pwf-nvim did not answer within %d seconds"):format(REQUEST_TIMEOUT_MS / 1000))
  end, REQUEST_TIMEOUT_MS)
end

--- Receives a reply from the child; not part of the plugin's public API.
--- @param id integer
--- @param reply { status: "ok"|"failed", value: any, message: string? }
function M.resolve(id, reply)
  if reply.status == "ok" then
    settle(id, nil, reply.value)
  else
    settle(id, reply.message or "pwf-nvim failed without a message")
  end
end

return M
