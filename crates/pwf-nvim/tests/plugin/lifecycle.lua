local client = require("pwf.client")

local function children()
  return vim.tbl_filter(function(channel)
    return channel.stream == "job" and channel.mode == "rpc"
  end, vim.api.nvim_list_chans())
end

local function timers_active()
  local count = 0
  vim.uv.walk(function(handle)
    if handle:get_type() == "timer" and handle:is_active() and not handle:is_closing() then
      count = count + 1
    end
  end)
  return count
end

local timers_before = timers_active()
local function assert_idle()
  assert(vim.wait(2000, function() return #children() == 0 end, 10), "pwf-nvim remained running")
  assert(timers_active() == timers_before, "request deadline timers remained active")
end

local callbacks = 0
local function listed(err, value)
  assert(not err, err)
  assert(value.hidden == 0)
  callbacks = callbacks + 1
end

client.request("list_records", { status = "active", limit = 10 }, listed)
client.request("list_records", { status = "active", limit = 10 }, listed)
assert(#children() == 1, "concurrent requests did not share a child")
assert(vim.wait(20000, function() return callbacks == 2 end, 10))
assert_idle()

-- Restart from the callback before the stopped child's exit notification is delivered.
local outcome = await(function(done)
  client.request("list_records", { status = "active", limit = 10 }, function(err)
    assert(not err, err)
    client.request("list_records", { status = "active", limit = 10 }, done)
  end)
end)
assert(not outcome.err, outcome.err)
assert_idle()

local rejected, finished = false, 0
for _ = 1, 16 do
  client.request("unknown_operation", {}, function(err)
    assert(err and err:find("unknown operation", 1, true), err)
    finished = finished + 1
  end)
end
client.request("list_records", { status = "active", limit = 10 }, function(err)
  assert(err and err:find("already serving 16 requests", 1, true), err)
  rejected = true
end)
assert(timers_active() == timers_before + 16, "too many deadline timers")
assert(vim.wait(20000, function() return rejected and finished == 16 end, 10))
assert_idle()

local failures = 0
for _ = 1, 2 do
  client.request("list_records", { status = "active", limit = 10 }, function(err)
    assert(err and err:find("pwf%-nvim exited"), err)
    failures = failures + 1
  end)
end
vim.fn.jobstop(children()[1].id)
assert(vim.wait(20000, function() return failures == 2 end, 10))
assert_idle()

outcome = await(function(done)
  client.request("list_records", { status = "active", limit = 10 }, done)
end)
assert(not outcome.err, outcome.err)
assert_idle()
assert(callbacks == 2 and failures == 2, "a request settled more than once")

local child_script = vim.fn.getcwd() .. "/silent-child.lua"
vim.fn.writefile({ "vim.wait(30000, function() return false end, 100)" }, child_script)
require("pwf").setup({ cmd = { vim.v.progpath, "--clean", "-n", "-l", child_script } })
outcome = await(function(done)
  client.request("list_records", { status = "active", limit = 10 }, done)
end)
assert(outcome.err and outcome.err:find("did not answer within 15 seconds", 1, true), outcome.err)
assert_idle()
return true
