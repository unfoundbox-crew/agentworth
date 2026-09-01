#!/usr/bin/env node

import path from 'node:path';
import { run } from '../lib/resolver.js';

// Both npm bin entries ('agentworth' and 'agwt', see package.json) point at this same
// script -- process.argv[1] carries the name the shell actually resolved (the .bin
// symlink), which is how we tell the two invocations apart without a second script.
const invokedAs = path.basename(process.argv[1] || '').replace(/\.(js|cjs|mjs)$/, '');

const exitCode = run(process.argv.slice(2), { invokedAs });
process.exit(exitCode);
