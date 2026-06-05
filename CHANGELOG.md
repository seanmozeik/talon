# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.3] - 2026-06-05

## Added

- Support for YAML frontmatter sequences containing commas during parsing.
- New `ranking` class for query results.

## Changed

- Graph is now rebuilt incrementally during sync for improved performance.
- Graph rebuild process has been optimized for faster execution.
- Text processing logic has been updated to support new features.

## Fixed

- MCP search no longer panics when encountering multibyte content.
- Yake backtrack mechanism now handles edge cases correctly.

