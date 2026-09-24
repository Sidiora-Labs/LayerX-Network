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
    // during formatting we need to prefix ids to avoid conflicts with other icons
    // this is not the case for building the sprite
    // so we use a separate config only with one additional plugin
    'prefixIds',
  ],
  js2svg: {
    indent: 2,
    pretty: true,
  },
};
