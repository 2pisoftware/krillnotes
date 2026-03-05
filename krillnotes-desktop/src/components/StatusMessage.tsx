// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2024-2026 TripleACS Pty Ltd t/a 2pi Software

interface StatusMessageProps {
  message: string;
  isError?: boolean;
}

function StatusMessage({ message, isError = false }: StatusMessageProps) {
  return (
    <div className={`mt-4 p-4 rounded-lg ${
      isError
        ? 'bg-red-500/10 border border-red-500/20 text-red-500'
        : 'bg-secondary'
    }`}>
      <p className="text-sm">{message}</p>
    </div>
  );
}

export default StatusMessage;
