--!nonstrict
-- Text Sync: reads text from a file and displays it in the bar widget.
-- Configurable refresh interval, text color, font, and max character width.

local filePath = noctalia.getConfig("file_path") or ""
local refreshInterval = noctalia.getConfig("refresh_interval") or 1000
local textColor = noctalia.getConfig("text_color") or "none"
local fontFamily = noctalia.getConfig("font_family") or ""
local maxChars = noctalia.getConfig("max_chars") or 30

local displayText = ""

local function readFileContent()
  if filePath == "" then
    displayText = ""
    return
  end
  local content, err = noctalia.readFile(filePath)
  if content then
    displayText = noctalia.string.trim(content)
  else
    noctalia.log("TextSync: read failed [" .. tostring(filePath) .. "]: " .. tostring(err))
    displayText = ""
  end
end

local function render()
  if filePath == "" then
    barWidget.setText(noctalia.tr("widget.no_file"))
    return
  end

  if displayText == "" then
    barWidget.setText("…")
    return
  end

  local text = displayText
  if maxChars > 0 then
    local charLen = utf8.len(text)
    if charLen and charLen > maxChars then
      local byteOffset = utf8.offset(text, maxChars + 1)
      if byteOffset then
        text = string.sub(text, 1, byteOffset - 1) .. "…"
      end
    end
  end

  barWidget.setText(text)

  if textColor ~= "" and textColor ~= "none" then
    barWidget.setColor(textColor)
  end

  if fontFamily ~= "" then
    barWidget.setFont(fontFamily)
  end
end

-- Initial read and render
readFileContent()
render()

function update()
  noctalia.setUpdateInterval(math.max(100, refreshInterval))
  readFileContent()
  render()
end

function onRightClick()
  noctalia.openSettings()
end