export function getUserHandle(email: string) {
  return email.split('@')[0];
}
