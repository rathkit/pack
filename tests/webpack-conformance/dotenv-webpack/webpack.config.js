const Dotenv = require('dotenv-webpack');
module.exports = { mode: 'production', target: 'node', plugins: [new Dotenv({ path: './.env', systemvars: false })] };
