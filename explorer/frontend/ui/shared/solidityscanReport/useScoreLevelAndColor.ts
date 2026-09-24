export default function useScoreLevelAndColor(score: number) {
  const greatScoreColor = { _light: 'green.600', _dark: 'green.400' };
  const averageScoreColor = { _light: 'purple.600', _dark: 'purple.400' };
  const lowScoreColor = { _light: 'red.600', _dark: 'red.400' };

  let scoreColor;
  let scoreLevel;
  if (score >= 80) {
    scoreColor = greatScoreColor;
    scoreLevel = 'GREAT';
  } else if (score >= 30) {
    scoreColor = averageScoreColor;
    scoreLevel = 'AVERAGE';
  } else {
    scoreColor = lowScoreColor;
    scoreLevel = 'LOW';
  }
  return { scoreColor, scoreLevel };
}
