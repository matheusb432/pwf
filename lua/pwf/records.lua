local client = require("pwf.client")
local M = {}

function M.context_paths()
  local paths = {}
  local name = vim.api.nvim_buf_get_name(0)
  if name ~= "" and vim.bo.buftype == "" then
    table.insert(paths, name)
  end
  table.insert(paths, vim.fn.getcwd())
  return paths
end

function M.list(query, callback)
  client.request("list_records", query, callback)
end

function M.resolve(listing, entry)
  local id, line = entry:match("^([^\t]+)\t(%d+)\t")
  local path = id and listing.files[id]
  if path then return { path = path, line = tonumber(line), col = 1 } end
end

function M.open(file, command)
  local ok, err = pcall(function()
    local stat = vim.uv.fs_stat(file.path)
    if not stat or stat.type ~= "file" then error("record file is no longer available: " .. file.path) end
    vim.cmd[command](vim.fn.fnameescape(file.path))
    vim.api.nvim_win_set_cursor(0, { math.min(file.line, vim.api.nvim_buf_line_count(0)), 0 })
    vim.cmd("normal! zvzz")
  end)
  return not ok and tostring(err) or nil
end

return M
