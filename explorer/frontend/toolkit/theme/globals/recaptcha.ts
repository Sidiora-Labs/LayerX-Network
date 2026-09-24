const styles = {
  '.grecaptcha-badge': {
    visibility: 'hidden',
  },
  'div:has(div):has(iframe[title="recaptcha challenge expires in two minutes"])': {
    '&::after': {
      content: `" "`,
      display: 'block',
      position: 'fixed',
      top: 0,
      left: 0,
      width: '100vw',
      height: '100vh',
      zIndex: 100000,
      bgColor: 'blackAlpha.300',
    },
  },
};

export default styles;
