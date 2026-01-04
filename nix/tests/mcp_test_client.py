#!/usr/bin/env python3
"""Simple MCP client for testing streamable HTTP transport."""

import asyncio
import json
import sys
from mcp import ClientSession
from mcp.client.streamable_http import streamablehttp_client


async def test_mcp_server(url: str) -> dict:
    """Test MCP server connectivity and basic operations."""
    results = {
        "initialize": False,
        "list_tools": False,
        "exec": False,
        "stream_logs": False,
        "exec_id": None,
        "errors": []
    }

    try:
        async with streamablehttp_client(url) as (read_stream, write_stream, _):
            async with ClientSession(read_stream, write_stream) as session:
                # Test 1: Initialize
                await session.initialize()
                results["initialize"] = True
                print("OK: initialize", file=sys.stderr)

                # Test 2: List tools
                tools = await session.list_tools()
                tool_names = [t.name for t in tools.tools]
                if "exec" in tool_names and "stream_logs" in tool_names:
                    results["list_tools"] = True
                    print(f"OK: list_tools - {tool_names}", file=sys.stderr)
                else:
                    results["errors"].append(f"Missing tools: {tool_names}")

                # Test 3: Execute command
                exec_result = await session.call_tool("exec", {
                    "cmd": "echo hello_mcp_test_12345",
                    "timeout": 10
                })
                if exec_result.content:
                    content_text = exec_result.content[0].text
                    exec_data = json.loads(content_text)
                    if "id" in exec_data:
                        results["exec"] = True
                        results["exec_id"] = exec_data["id"]
                        print(f"OK: exec - id={exec_data['id']}", file=sys.stderr)

                # Wait for command to complete
                await asyncio.sleep(1)

                # Test 4: Stream logs
                if results["exec_id"]:
                    stream_result = await session.call_tool("stream_logs", {
                        "id": results["exec_id"],
                        "offset": 0
                    })
                    if stream_result.content:
                        content_text = stream_result.content[0].text
                        if "hello_mcp_test_12345" in content_text:
                            results["stream_logs"] = True
                            print("OK: stream_logs - found output", file=sys.stderr)
                        else:
                            results["errors"].append(f"Output not found in: {content_text[:100]}")

    except Exception as e:
        results["errors"].append(str(e))
        print(f"ERROR: {e}", file=sys.stderr)

    return results


async def main():
    import argparse
    parser = argparse.ArgumentParser()
    parser.add_argument("--url", default="http://127.0.0.1:19222/", help="MCP server URL")
    parser.add_argument("--json", action="store_true", help="Output JSON results")
    args = parser.parse_args()

    results = await test_mcp_server(args.url)

    if args.json:
        print(json.dumps(results))

    # Exit with error if any test failed
    all_passed = all([
        results["initialize"],
        results["list_tools"],
        results["exec"],
        results["stream_logs"]
    ])

    if all_passed:
        print("All tests passed!", file=sys.stderr)
        sys.exit(0)
    else:
        print(f"Some tests failed: {results}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
