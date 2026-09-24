const useFeatureValue = (name, fallback) => {
  try {
    const value = JSON.parse(localStorage.getItem(`pw_feature:${ name }`));
    if (value === null) {
      throw new Error();
    }
    return { isLoading: false, value };
  } catch (error) {
    return { isLoading: false, value: fallback };
  }
};

export default useFeatureValue;
