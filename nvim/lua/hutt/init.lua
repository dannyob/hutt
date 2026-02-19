-- hutt.nvim — Neovim plugin for the hutt email client.
--
-- Provides compose-mode integration: filetype detection, buffer setup,
-- address completion, send/discard commands, and keybindings.

local compose = require("hutt.compose")
local complete = require("hutt.complete")
local send = require("hutt.send")

local M = {}

--- Default configuration.  Users can override via require("hutt").setup({...}).
M.config = {
  -- Signature text (without the "-- \n" delimiter, which is added automatically).
  -- Set to nil or "" to disable.
  signature = nil,
}

--- Set up the plugin: commands, autocommands, and keymaps.
--- @param opts table|nil  Optional overrides for M.config fields.
function M.setup(opts)
  M.config = vim.tbl_deep_extend("force", M.config, opts or {})

  -- -----------------------------------------------------------------
  -- User commands (available globally, but only meaningful in compose
  -- buffers — the functions are safe to call anywhere).
  -- -----------------------------------------------------------------
  vim.api.nvim_create_user_command("HuttSend", function()
    send.send()
  end, { desc = "Hutt: send the composed message" })

  vim.api.nvim_create_user_command("HuttDiscard", function()
    send.discard()
  end, { desc = "Hutt: discard the composed message" })

  -- -----------------------------------------------------------------
  -- Autocommand: detect hutt compose files and configure the buffer.
  --
  -- Hutt writes temp files matching /tmp/hutt-compose-*.eml (or the
  -- OS temp dir equivalent).  We match on the pattern and also on
  -- any *.eml file inside a directory whose name starts with "hutt-".
  -- -----------------------------------------------------------------
  local augroup = vim.api.nvim_create_augroup("Hutt", { clear = true })

  vim.api.nvim_create_autocmd({ "BufRead", "BufNewFile" }, {
    group = augroup,
    pattern = {
      "*/hutt-compose-*.eml",
      "*/hutt-*.eml",
    },
    desc = "Set up hutt compose buffer",
    callback = function()
      -- Set the filetype to mail so syntax highlighting works.
      vim.bo.filetype = "mail"

      -- Buffer options for composing email.
      compose.setup_buffer()

      -- Enable address completion on To/Cc/Bcc header lines.
      vim.bo.omnifunc = "v:lua.require'hutt.complete'.omnifunc"

      -- Append signature if configured.
      if M.config.signature and M.config.signature ~= "" then
        compose.add_signature(M.config.signature)
      end

      -- Buffer-local keymaps.
      local buf = vim.api.nvim_get_current_buf()
      vim.keymap.set("n", "<leader>s", "<cmd>HuttSend<cr>", {
        buffer = buf,
        desc = "Hutt: send message",
      })
      vim.keymap.set("n", "<leader>d", "<cmd>HuttDiscard<cr>", {
        buffer = buf,
        desc = "Hutt: discard message",
      })

      -- Place cursor in the body, after headers.
      -- Defer so the buffer content is fully loaded.
      vim.schedule(function()
        compose.cursor_to_body()
      end)
    end,
  })
end

return M
