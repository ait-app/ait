export function canPreviewPdf(): boolean {
  return typeof navigator !== "undefined" && navigator.pdfViewerEnabled === true;
}

export function FilePdfPreview({ uri }: { uri: string }) {
  return (
    <iframe
      title="PDF"
      src={uri}
      referrerPolicy="no-referrer"
      style={{ flex: 1, width: "100%", border: 0, minHeight: 0 }}
    />
  );
}
