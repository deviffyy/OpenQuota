export function beginDrag(event: DragEvent, title: string, detail?: string) {
  const transfer = event.dataTransfer;
  if (!transfer) return;
  transfer.effectAllowed = 'move';

  const preview = document.createElement('div');
  preview.className = 'drag-lift-preview';
  const heading = document.createElement('strong');
  heading.textContent = title;
  preview.append(heading);
  if (detail) {
    const caption = document.createElement('span');
    caption.textContent = detail;
    preview.append(caption);
  }
  document.body.append(preview);
  transfer.setDragImage(preview, 22, preview.offsetHeight / 2);
  setTimeout(() => preview.remove());
}
