module.exports = {
  plugins: [
    {
      name: 'preset-default',
      params: {
        overrides: {
          removeViewBox: false,
          removeHiddenElems: false,
        },
      },
    },
    'removeDimensions',
  ],
  js2svg: {
    indent: 2,
    pretty: true,
  },
};
