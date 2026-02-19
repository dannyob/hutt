-- hutt/complete.lua — Email address completion via mu cfind.

local M = {}

--- Fallback: parse mu cfind --format=mutt-ab output.
--- Lines are: address<TAB>name (first line is a header we skip).
--- @param pattern string
--- @return table[]
local function mu_cfind_mutt_ab(pattern)
  local cmd = string.format("mu cfind --format=mutt-ab %s 2>/dev/null", vim.fn.shellescape(pattern))
  local output = vim.fn.system(cmd)

  if vim.v.shell_error ~= 0 or output == "" then
    return {}
  end

  local results = {}
  local first = true
  for line in output:gmatch("[^\r\n]+") do
    if first then
      first = false -- skip the header line
    else
      local addr, name = line:match("^([^\t]+)\t(.*)$")
      if addr then
        local display
        if name and name ~= "" then
          display = string.format("%s <%s>", name, addr)
        else
          display = addr
        end
        table.insert(results, {
          word = display,
          abbr = display,
          menu = "[mu]",
        })
      end
    end
  end
  return results
end

--- Query mu's contact database for addresses matching `pattern`.
--- Returns a list of tables with `word`, `abbr`, and `menu` fields
--- suitable for nvim's omnifunc completion.
--- @param pattern string  The prefix/substring to search for.
--- @return table[]
local function mu_cfind(pattern)
  if not pattern or pattern == "" then
    return {}
  end

  -- mu cfind --format=json outputs a JSON array of objects with
  -- "name" and "address" keys (or just "address" when name is empty).
  -- Older mu versions may not support --format=json; we fall back to
  -- --format=mutt-ab which gives tab-separated "address<TAB>name" lines.
  local cmd = string.format("mu cfind --format=json %s 2>/dev/null", vim.fn.shellescape(pattern))
  local output = vim.fn.system(cmd)

  if vim.v.shell_error ~= 0 or output == "" then
    -- Try mutt-ab fallback
    return mu_cfind_mutt_ab(pattern)
  end

  local ok, decoded = pcall(vim.json.decode, output)
  if not ok or type(decoded) ~= "table" then
    return mu_cfind_mutt_ab(pattern)
  end

  local results = {}
  for _, entry in ipairs(decoded) do
    local addr = entry.address or entry.email or ""
    local name = entry.name or ""
    local display
    if name ~= "" then
      display = string.format("%s <%s>", name, addr)
    else
      display = addr
    end
    table.insert(results, {
      word = display,
      abbr = display,
      menu = "[mu]",
    })
  end
  return results
end

--- Omnifunc for email address completion.
--- Set via `vim.bo.omnifunc = "v:lua.require'hutt.complete'.omnifunc"`
--- or by calling `vim.bo.omnifunc = "v:lua.HuttComplete"`.
---
--- @param findstart number  1 = find the start column; 0 = return matches.
--- @param base string       The text to complete (only when findstart == 0).
--- @return number|table
function M.omnifunc(findstart, base)
  if findstart == 1 then
    -- We only complete on header lines (To:, Cc:, Bcc:).
    local line = vim.api.nvim_get_current_line()
    local cursor_col = vim.api.nvim_win_get_cursor(0)[2] -- 0-indexed byte offset

    -- Check we're on a header line that takes addresses.
    local header = line:match("^(%a[%a-]*):")
    if not header then
      return -3 -- cancel completion
    end
    local lh = header:lower()
    if lh ~= "to" and lh ~= "cc" and lh ~= "bcc" then
      return -3
    end

    -- Find start of the current address token. Addresses are separated
    -- by commas, so scan backwards from the cursor to the last comma
    -- or colon.
    local start = cursor_col
    while start > 0 do
      local ch = line:sub(start, start)
      if ch == "," or ch == ":" then
        start = start + 1
        break
      end
      start = start - 1
    end
    -- Skip leading whitespace.
    while start <= cursor_col and line:sub(start + 1, start + 1) == " " do
      start = start + 1
    end
    return start
  else
    -- findstart == 0: return matching completions.
    return mu_cfind(base)
  end
end

return M
