// Clip the supplied 750x1148 original at its white QR frame. No QR pixels are
// reconstructed, resampled into a new asset, or covered with a logo.
export function SupportQr() {
  return <svg className="support-qr-only" viewBox="154 356 442 442" role="img" aria-label="赞赏二维码" xmlns="http://www.w3.org/2000/svg">
    <image href="/support-qr.jpg" x="0" y="0" width="750" height="1148" />
  </svg>;
}
