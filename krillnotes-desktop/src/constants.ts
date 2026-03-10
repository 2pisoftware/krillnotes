import { TrustLevel } from './types';

export const TRUST_LEVELS: { value: TrustLevel; label: string }[] = [
  { value: 'Tofu', label: 'Trust on first use' },
  { value: 'CodeVerified', label: 'Code verified (phone/video)' },
  { value: 'Vouched', label: 'Vouched for by another contact' },
  { value: 'VerifiedInPerson', label: 'Verified in person (highest)' },
];
