#!/usr/bin/env node
'use strict';
// npm create accrete@VERSION -- directory [native init options]
// Initialization is exclusively the Rust kernel's responsibility.
require('@accrete/cli/bin/accrete.cjs').launch(['init', ...process.argv.slice(2)]);
