# Protocol

`talon --mcp` will speak standard MCP over stdio. The scaffold does not implement the MCP loop yet.

One-shot CLI commands are normal CLI invocations that read Talon's host config and operate on Talon's own DB/index. Container usage routes through ultraclaw's normal host shim path, exactly like other host tools; it does not involve MCP.
