---
title: "How the Converter Works"
description: "A tour through the Astro frontend, Rust Worker backend, and external rendering service."
publishDate: 2025-02-01
author: "Universal Converter Team"
tags: ["architecture", "engineering"]
---

# Architecture

The service is split into three pieces, each chosen for the property it
brings to the request path.

## 1. Astro on Cloudflare Pages

The marketing site, blog, and editor UI live in an Astro app deployed to
Pages. Most pages are static; the editor is a React island that calls
`/api/convert` and streams results back.

## 2. Rust Worker

A small Rust crate compiled to Wasm via `worker-build` handles the
`/api/*` namespace. It parses the inbound payload, normalizes the
content (Markdown via `pulldown-cmark`, JSON via `serde_json`, etc.),
and either returns HTML directly or forwards a render request.

## 3. External rendering service

Headless Chrome cannot run inside a Workers isolate, so the worker
dispatches an HTTPS request to an external rendering service
(Browserless, Puppeteer-compatible) when PDF output is requested. The
result is streamed back to the client.
