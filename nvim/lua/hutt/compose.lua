-- hutt/compose.lua — Buffer setup for composing email in neovim.

local M = {}

--- Set buffer-local options appropriate for composing email.
--- Called automatically when a hutt compose file is opened.
function M.setup_buffer()
  local bo = vim.bo
  local wo = vim.wo

  -- Hard-wrap at 72 columns (RFC 2822 recommendation).
  bo.textwidth = 72

  -- Format options:
  --   t = auto-wrap text using textwidth
  --   c = auto-wrap comments
  --   q = allow formatting of comments with "gq"
  --   r = insert comment leader after <Enter> in insert mode
  --   n = recognise numbered lists
  --   1 = don't break after a one-letter word
  bo.formatoptions = "tcqrn1"

  -- Use spaces, 4-wide indent (common for plain-text email).
  bo.expandtab = true
  bo.shiftwidth = 4
  bo.tabstop = 4

  -- Spell-checking is handy for prose.
  wo.spell = true
  wo.spelllang = "en"

  -- Soft-wrap long lines in the display so nothing is hidden.
  wo.wrap = true
  wo.linebreak = true
end

--- Position the cursor at the start of the message body.
--- The body begins after the first blank line (which separates headers
--- from body in RFC 2822 format).
function M.cursor_to_body()
  local lines = vim.api.nvim_buf_get_lines(0, 0, -1, false)
  for i, line in ipairs(lines) do
    if line == "" then
      -- Place cursor on the line after the blank separator, or on the
      -- blank line itself if there is nothing after it.
      local target = math.min(i + 1, #lines)
      vim.api.nvim_win_set_cursor(0, { target, 0 })
      return
    end
  end
  -- No blank line found; just go to the end.
  vim.api.nvim_win_set_cursor(0, { #lines, 0 })
end

--- Append a signature to the buffer.
--- @param sig string|nil  The signature text (without the "-- \n" delimiter).
---   If nil, nothing is appended.
function M.add_signature(sig)
  if not sig or sig == "" then
    return
  end
  local buf = vim.api.nvim_get_current_buf()
  local count = vim.api.nvim_buf_line_count(buf)
  local sig_lines = { "", "-- " }
  for line in sig:gmatch("[^\r\n]+") do
    table.insert(sig_lines, line)
  end
  vim.api.nvim_buf_set_lines(buf, count, count, false, sig_lines)
end

return M
