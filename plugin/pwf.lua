-- Registers pwf commands and <Plug> mappings; the plugin modules load on first use.
if vim.g.loaded_pwf then
  return
end
vim.g.loaded_pwf = true

local subcommands = {
  tasks = function() require("pwf").tasks() end,
  notes = function() require("pwf").notes() end,
}

vim.api.nvim_create_user_command("Pwf", function(command)
  local name = command.fargs[1] or "tasks"
  local run = subcommands[name]
  if not run then
    vim.notify(("pwf: unknown subcommand %q"):format(name), vim.log.levels.ERROR)
    return
  end
  run()
end, {
  nargs = "?",
  desc = "Find pwf tasks, notes, and contents",
  complete = function(lead)
    local names = vim.tbl_filter(function(name)
      return vim.startswith(name, lead)
    end, vim.tbl_keys(subcommands))
    table.sort(names)
    return names
  end,
})

vim.keymap.set("n", "<Plug>(pwf-tasks)", subcommands.tasks, { desc = "pwf: open a task" })
vim.keymap.set("n", "<Plug>(pwf-notes)", subcommands.notes, { desc = "pwf: open a note" })
