/** Three strip rows: things said, what the model spent, actions taken. */
export type EventGroup = 'messages' | 'model' | 'tools';

export function getEventGroup(type: string): EventGroup {
  if (type === 'model_invocation') return 'model';
  if (
    type === 'tool_call' ||
    type === 'shell_command' ||
    type === 'file_action' ||
    type === 'tool_result'
  ) {
    return 'tools';
  }
  return 'messages';
}
