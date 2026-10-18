-- Bounded local sensor reader. Contributions are declared in manifest.toml.
local owner = "extension.sensor.reader."

local function parse(ctx)
  local bytes, eof = fyle.read(ctx.source, 0, ctx.demand.max_bytes)
  local partial = not eof
  if not eof then
    -- The final unterminated line may be cut off by the byte budget.
    bytes = bytes:match("^(.*\n)") or ""
  end
  local header = false
  local samples = {}
  for line in (bytes .. "\n"):gmatch("(.-)\n") do
    line = line:gsub("\r$", "")
    if not header then
      if line ~= "FYLE-SENSOR 1" then
        error("expected FYLE-SENSOR 1 header")
      end
      header = true
    elseif line:match("%S") then
      if #samples >= ctx.demand.max_items then
        partial = true
        break
      end
      local seconds, metres = line:match("^%s*(%S+)%s+(%S+)%s*$")
      seconds, metres = tonumber(seconds), tonumber(metres)
      if seconds == nil or metres == nil or seconds ~= seconds or metres ~= metres
          or seconds < 0 or metres < 0
          or seconds == math.huge or metres == math.huge then
        error("expected finite nonnegative seconds and metres")
      end
      local previous = samples[#samples]
      if previous == nil then
        if seconds ~= 0 or metres ~= 0 then
          error("first sample must be 0 0")
        end
      elseif seconds <= previous.seconds or metres < previous.metres then
        error("sample times must increase and distances must not decrease")
      end
      samples[#samples + 1] = { seconds = seconds, metres = metres }
    end
  end
  return samples, partial
end

local function quantity(value, unit)
  return { kind = "quantity", value = value, unit = unit }
end

local function completeness(partial)
  if partial then return { kind = "lower_bound" } end
  return { kind = "complete" }
end

local function distance(ctx)
  local samples, partial = parse(ctx)
  if #samples == 0 then return { state = "absent" } end
  return {
    state = "ready", field = owner .. "distance",
    value = quantity(samples[#samples].metres, "metres"),
    completeness = completeness(partial),
  }
end

local function summary(ctx)
  local samples, partial = parse(ctx)
  if #samples == 0 then return { state = "absent" } end
  local last = samples[#samples]
  return { state = "ready", fields = {
    { state = "ready", field = owner .. "duration",
      value = quantity(last.seconds, "seconds"), completeness = completeness(partial) },
    { state = "ready", field = owner .. "distance",
      value = quantity(last.metres, "metres"), completeness = completeness(partial) },
  } }
end

local function graph(ctx)
  local samples, partial = parse(ctx)
  if #samples == 0 then return { state = "absent" } end
  local last = samples[#samples]
  local points, rows = {}, {}
  for i, sample in ipairs(samples) do
    points[i] = {
      x = 300 * (sample.seconds / (last.seconds > 0 and last.seconds or 1)),
      y = 140 - 140 * (sample.metres / (last.metres > 0 and last.metres or 1)),
    }
    if i <= 16 then
      rows[i] = { quantity(sample.seconds, "seconds"), quantity(sample.metres, "metres") }
    end
  end
  local paths = {}
  if #points >= 2 then
    paths[1] = { role = "accent", weight = "normal", points = points }
  end
  local note = "Elapsed seconds → cumulative metres"
  if partial then note = note .. " · partial input; totals are lower bounds" end
  if #samples > 16 then note = note .. " · table shows first 16 samples" end
  local scene_completeness = { kind = "complete" }
  if partial then
    scene_completeness = { kind = "truncated", reason = "sensor input limit" }
  end
  return { state = "ready", completeness = scene_completeness, scene = {
    version = 1,
    root = { kind = "stack", id = "sensor", axis = "column", gap = "normal", children = {
      { kind = "text", id = "title", role = "heading", text = "Sensor distance over time" },
      { kind = "text", id = "summary", role = "body",
        text = string.format("%g seconds · %g metres · %d samples", last.seconds, last.metres, #samples) },
      { kind = "canvas", id = "profile", width = 300, height = 140, paths = paths },
      { kind = "text", id = "note", role = "muted", text = note },
      { kind = "table", id = "samples", columns = {
        { id = "seconds", label = "Elapsed", kind = { kind = "quantity", unit = "seconds" } },
        { id = "metres", label = "Distance", kind = { kind = "quantity", unit = "metres" } },
      }, rows = rows },
    } },
  } }
end

local function contribution(read)
  return {
    applies = function(ctx) return ctx.name:lower():match("%.sens$") ~= nil end,
    read = read,
  }
end

return {
  [owner .. "distance-cell"] = contribution(distance),
  [owner .. "summary"] = contribution(summary),
  [owner .. "graph"] = contribution(graph),
}
