--- Inserts `[[ID]]` task references into editor buffers.
local M = {}

--- @class pwf.ReferenceTarget
--- @field win integer
--- @field buf integer
--- @field row integer 1-based line
--- @field col integer 0-based byte column where the reference starts
--- @field insert_mode boolean

--- Captures where a reference goes before a picker takes focus.
--- @return pwf.ReferenceTarget
function M.target()
  local win = vim.api.nvim_get_current_win()
  local row, col = unpack(vim.api.nvim_win_get_cursor(win))
  return {
    win = win,
    buf = vim.api.nvim_win_get_buf(win),
    row = row,
    col = col,
    insert_mode = vim.api.nvim_get_mode().mode:sub(1, 1) == "i",
  }
end

--- Inserts `[[id]]` at the target and moves the cursor past it. A target captured in Insert mode
--- resumes Insert mode after the reference.
--- @param id string
--- @param target pwf.ReferenceTarget
--- @return boolean inserted, string? err
function M.insert(id, target)
  if not vim.api.nvim_buf_is_valid(target.buf) then
    return false, "the buffer for the reference was closed"
  end
  local text = ("[[%s]]"):format(id)
  local row = target.row - 1
  local inserted, err =
    pcall(vim.api.nvim_buf_set_text, target.buf, row, target.col, row, target.col, { text })
  if not inserted then
    return false, tostring(err)
  end
  if not vim.api.nvim_win_is_valid(target.win) or vim.api.nvim_win_get_buf(target.win) ~= target.buf then
    return true
  end
  vim.api.nvim_set_current_win(target.win)
  local end_col = target.col + #text
  if not target.insert_mode then
    vim.api.nvim_win_set_cursor(target.win, { target.row, end_col - 1 })
    return true
  end
  local line = vim.api.nvim_buf_get_lines(target.buf, row, target.row, true)[1]
  if end_col >= #line then
    vim.cmd("startinsert!")
  else
    vim.api.nvim_win_set_cursor(target.win, { target.row, end_col })
    vim.cmd("startinsert")
  end
  return true
end

return M
