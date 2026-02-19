-- hutt/send.lua — Send / discard helpers for the hutt compose flow.
--
-- Hutt's compose mechanism works like this:
--   1. Hutt writes a temp file at /tmp/hutt-compose-<pid>.eml
--   2. It launches $EDITOR (nvim) on that file
--   3. On editor exit it checks whether the file's mtime changed
--      - mtime changed  -> read the file and send via SMTP
--      - mtime unchanged -> treat as "cancelled"
--
-- "Send" therefore means: write the buffer (updating mtime) and quit.
-- "Discard" means: quit without writing (mtime stays the same).

local M = {}

--- Mark the compose buffer as ready to send.
--- Writes the buffer to disk (which updates mtime), then closes it.
function M.send()
  -- Write and quit. If the buffer was never modified neovim would
  -- normally skip the write, but we force it so the mtime changes.
  vim.cmd("write!")
  vim.cmd("bdelete!")
end

--- Discard the compose buffer without sending.
--- Closes without writing so the mtime is unchanged; hutt sees this
--- as a cancellation.
function M.discard()
  -- Mark buffer as unmodified so :bdelete! won't prompt, then close.
  vim.bo.modified = false
  vim.cmd("bdelete!")
end

return M
