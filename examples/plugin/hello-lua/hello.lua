-- hello.lua — example jvim plugin using the Neovim-compatible Lua API
--
-- Load via config.toml:
--   plugins = ["mlua-wasm", "hello"]
--
-- Or at runtime:
--   :plugin load mlua-wasm
--   :plugin load hello

-- Greet command: :plugin hello.Greet()
vim.api.nvim_create_user_command("Greet", function(opts)
    vim.notify("Hello from Lua!")
end, {})

-- Info command: shows cursor position and current line content
vim.api.nvim_create_user_command("Info", function(opts)
    local buf = vim.api.nvim_get_current_buf()
    local win = vim.api.nvim_get_current_win()
    local pos = vim.api.nvim_win_get_cursor(win)
    local row = pos[1]
    local col = pos[2]

    local lines = vim.api.nvim_buf_get_lines(buf, row - 1, row, false)
    local line = lines[1] or ""

    vim.notify(string.format("line %d col %d: %s", row, col, line))
end, {})

-- <leader>h → greet
vim.keymap.set("n", "<leader>h", function()
    vim.notify("Hello from <leader>h!")
end, { desc = "Say hello" })

-- <leader>i → info about current position
vim.keymap.set("n", "<leader>i", function()
    local buf = vim.api.nvim_get_current_buf()
    local win = vim.api.nvim_get_current_win()
    local pos = vim.api.nvim_win_get_cursor(win)
    local lines = vim.api.nvim_buf_get_lines(buf, pos[1] - 1, pos[1], false)
    vim.notify(string.format("[%d,%d] %s", pos[1], pos[2], lines[1] or ""))
end, { desc = "Show cursor info" })

print("hello.lua loaded")
