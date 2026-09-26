local records = require("pwf.records")
local cwd = vim.fn.getcwd()
local listing = await(function(done)
  records.list({ context_paths = records.context_paths(), status = "active", limit = 100 }, done)
end)
assert(not listing.err, listing.err)
local content, note
for _, entry in ipairs(listing.value.entries) do
  if entry:find("body-only-needle", 1, true) then content = records.resolve(listing.value, entry) end
  if entry:find("[note]", 1, true) then note = records.resolve(listing.value, entry) end
end
assert(content and note)
assert(not records.open(content, "edit"))
local opened, cursor = vim.api.nvim_buf_get_name(0), vim.api.nvim_win_get_cursor(0)
local scoped = await(function(done)
  records.list({ context_paths = records.context_paths(), status = "active", limit = 100 }, done)
end)
vim.o.hidden = false
vim.api.nvim_buf_set_lines(0, -1, -1, true, { "unsaved text" })
local protected = records.open(note, "edit")
local unsaved = vim.api.nvim_buf_get_lines(0, -2, -1, true)[1]
assert(not records.open(note, "split"))
local missing = records.open({ path = note.path .. ".missing", line = 1 }, "edit")
return {
  listing = listing, opened = opened, cursor = cursor, scoped = scoped,
  protected = protected, unsaved = unsaved, missing = missing,
  cwd_unchanged = vim.fn.getcwd() == cwd,
}
