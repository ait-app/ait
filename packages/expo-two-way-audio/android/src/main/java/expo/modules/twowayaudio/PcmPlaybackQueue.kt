package expo.modules.twowayaudio

/** Bounded PCM16 queue. The owner serializes pump/enqueue/clear with device pause/flush. */
internal class PcmPlaybackQueue(
    private val write: (ByteArray, Int, Int) -> Int,
    private val position: () -> Int,
    private val complete: (String, String?) -> Unit,
) {
    private data class Packet(val data: ByteArray, val id: String?, var offset: Int = 0)
    private data class Submitted(val packet: Packet, val endFrame: Long)
    private val waiting = ArrayDeque<Packet>()
    private val submitted = ArrayDeque<Submitted>()
    private var bytes = 0
    private var writtenFrames = 0L
    private var head = 0L
    private var wraps = 0L

    val isNotEmpty: Boolean get() = waiting.isNotEmpty() || submitted.isNotEmpty()

    fun enqueue(data: ByteArray, id: String?): Boolean {
        if (data.isEmpty() || data.size % 2 != 0 || data.size > 512 * 1024 ||
            waiting.size + submitted.size >= 32 || bytes + data.size > 1024 * 1024) return false
        waiting.addLast(Packet(data, id))
        bytes += data.size
        return true
    }

    /** Advance using nonblocking device writes and its consumed-frame counter, never elapsed time. */
    fun pump(): Boolean {
        val nextHead = position().toLong() and 0xffffffffL
        if (nextHead < head) wraps += 1L shl 32
        head = nextHead
        while (submitted.firstOrNull()?.endFrame?.let { it <= wraps + head } == true) {
            val packet = submitted.removeFirst().packet
            bytes -= packet.data.size
            packet.id?.let { complete(it, null) }
        }
        val packet = waiting.firstOrNull() ?: return true
        val remaining = packet.data.size - packet.offset
        val written = write(packet.data, packet.offset, remaining)
        if (written < 0 || written > remaining || written % 2 != 0) {
            val ids = submitted.mapNotNull { it.packet.id } + waiting.mapNotNull { it.id }
            clear()
            ids.forEach { complete(it, "Audio device write failed") }
            return false
        }
        packet.offset += written
        writtenFrames += written / 2
        if (packet.offset == packet.data.size) {
            waiting.removeFirst()
            submitted.addLast(Submitted(packet, writtenFrames))
        }
        return true
    }

    /** Called together with AudioTrack.pause/flush, which reset its playback head to zero. */
    fun clear() {
        waiting.clear()
        submitted.clear()
        bytes = 0
        writtenFrames = 0
        head = 0
        wraps = 0
    }
}
