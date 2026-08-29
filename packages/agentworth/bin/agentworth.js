#!/usr/bin/env node

import { run } from '../lib/resolver.js';

const exitCode = run(process.argv.slice(2));
process.exit(exitCode);
