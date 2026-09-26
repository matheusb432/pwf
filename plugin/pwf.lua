-- Registers pwf commands and <Plug> mappings; the plugin modules load on first use.
if vim.g.loaded_pwf then
  return
end
vim.g.loaded_pwf = true

local subcommands = {
  tasks = function()
    require("pwf").tasks()
  end,
  reference = function()
    require("pwf").insert_reference()
  end,
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
  desc = "Open a pwf picker",
  complete = function(lead)
    local names = vim.tbl_filter(function(name)
      return vim.startswith(name, lead)
    end, vim.tbl_keys(subcommands))
    table.sort(names)
    return names
  end,
})

vim.keymap.set("n", "<Plug>(pwf-tasks)", subcommands.tasks, { desc = "pwf: open a task" })
vim.keymap.set(
  { "n", "i" },
  "<Plug>(pwf-insert-reference)",
  subcommands.reference,
  { desc = "pwf: insert a task reference" }
)
