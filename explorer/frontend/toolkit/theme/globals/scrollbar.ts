const scrollbar = {
  'body *::-webkit-scrollbar': {
    width: '20px',
  },
  'body *::-webkit-scrollbar-track': {
    backgroundColor: 'transparent',
  },
  'body *::-webkit-scrollbar-thumb': {
    backgroundColor: '{colors.global.scrollbar.thumb}',
    borderRadius: '20px',
    border: `8px solid rgba(0,0,0,0)`,
    backgroundClip: 'content-box',
    minHeight: '32px',
  },
  'body *::-webkit-scrollbar-button': {
    display: 'none',
  },
  'body *::-webkit-scrollbar-corner': {
    backgroundColor: 'transparent',
  },
  'body *::-webkit-resizer': {
    // FIXME for dark mode we need to use a different image - /static/resizer_dark.png
    backgroundImage: 'url(/static/resizer_light.png)',
    backgroundSize: '20px',
  },
  'body *': {
    scrollbarWidth: 'thin',
    scrollbarColor: `{colors.global.scrollbar.thumb} transparent`,
  },
};

export default scrollbar;
