const useReCaptcha = () => {
  return {
    ref: { current: null },
    executeAsync: () => Promise.resolve('recaptcha_token'),
    fetchProtectedResource: async(fetcher) => {
      const result = await fetcher();
      return result;
    },
  };
};

export default useReCaptcha;
