---
title: "Welcome to the Universal Document Converter"
description: "Introducing a fast, edge-rendered conversion engine powered by Rust and Cloudflare Workers."
publishDate: 2025-01-15
author: "Universal Converter Team"
tags: ["announcement", "rust", "cloudflare"]
---

# Welcome

The **Universal Document Converter** turns Markdown, HTML, JSON, and XML into
clean, structured documents — including PDF — from a single API call.

## Why edge?

By running parsing in Rust on Cloudflare Workers, every request stays close to
the user. Cold starts are measured in milliseconds, and the CPU envelope of a
Worker is plenty to normalize even large documents.

## What's next

- Streamed conversions for multi-megabyte payloads
- Webhook delivery for long-running PDF renders
- Self-hosted rendering adapter (BYO Browserless / Chromium)

Stay tuned.
