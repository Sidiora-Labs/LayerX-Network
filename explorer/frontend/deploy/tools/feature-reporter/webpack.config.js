const path = require('path');
module.exports = {
  mode: 'production',
  target: 'node',
  entry: path.resolve(__dirname, '/entry.js'),
  resolve: {
    extensions: [ '.js' ],
  },
  output: {
    filename: 'index.js',
    path: path.resolve(__dirname),
  },
};
