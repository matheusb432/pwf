require("pwf").setup(vim.tbl_extend("force", require("pwf").config, { task_scope = "global" }))
local views, options, transform_search = {}, nil, nil
local function matches(entries, opts, query)
  local search = transform_search and transform_search({ query }) or query
  local command = { "fzf", "--filter=" .. search }
  for flag, value in pairs(opts.fzf_opts) do
    if value == true then table.insert(command, flag)
    elseif value ~= false then table.insert(command, flag .. "=" .. value) end
  end
  local result = vim.system(command, {
    stdin = table.concat(entries, "\n") .. "\n", text = true,
    env = { FZF_DEFAULT_OPTS = "", FZF_DEFAULT_OPTS_FILE = "" },
  }):wait(5000)
  assert(result.code == 0 or result.code == 1, result.stderr)
  return vim.split(result.stdout, "\n", { trimempty = true })
end
local listings = setmetatable({}, { __mode = "v" })
local listing_count = 0
local records = require("pwf.records")
local list = records.list
records.list = function(query, done)
  list(query, function(err, listing)
    listing_count = listing_count + 1
    listings[listing_count] = listing
    done(err, listing)
  end)
end
package.loaded["fzf-lua"] = {
  shell = { stringify_data = function(fn)
    transform_search = fn
    return "test-search-transform"
  end },
  fzf_exec = function(contents, opts)
    local entries = {}
    contents(nil, function(rows) if rows then vim.list_extend(entries, rows) end end)
    options = opts
    if not opts.keymap then transform_search = nil end
    local queries = {}
    for _, query in ipairs({ "nama1", "NaMa1", "NAMA-0001", "nama-1", "nama0001", "nama12", "nama", "12", "title-only-needle", "body-only-needle", "note-body-needle" }) do
      queries[query] = matches(entries, opts, query)
    end
    table.insert(views, { entries = entries, queries = queries, prompt = opts.prompt, query = opts.query, title = opts.winopts.title })
  end,
}
require("pwf.picker").open()
assert(vim.wait(20000, function() return #views == 1 end, 10))
assert(options.actions["alt-g"])
options.winopts.on_close()
options.actions["ctrl-g"].fn({}, { last_query = "needle" })
assert(vim.wait(20000, function() return #views == 2 end, 10))
options.winopts.on_close()
options.actions["ctrl-g"].fn({}, { last_query = "body-only-needle" })
assert(vim.wait(20000, function() return #views == 3 end, 10))
assert(listing_count == 1)
options.winopts.on_close()
options.actions["alt-g"].fn({}, { last_query = "nama1" })
assert(vim.wait(20000, function() return #views == 4 end, 10))
assert(listing_count == 2)
options.winopts.on_close()
assert(vim.wait(2000, function()
  collectgarbage("collect")
  return next(listings) == nil
end, 10), "closed pickers retained their listings")
return views
