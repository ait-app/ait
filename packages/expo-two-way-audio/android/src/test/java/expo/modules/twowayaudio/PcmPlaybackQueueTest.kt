package expo.modules.twowayaudio

import org.junit.Assert.*
import org.junit.Test

class PcmPlaybackQueueTest {
    private class Device {
        var head = 0
        var limit = Int.MAX_VALUE
        val samples = mutableListOf<Byte>()
        val completed = mutableListOf<Pair<String, String?>>()
        val queue = PcmPlaybackQueue(
            write = { data, offset, length ->
                val count = minOf(length, limit)
                if (count > 0) samples.addAll(data.slice(offset until offset + count))
                count
            },
            position = { head },
            complete = { id, error -> completed.add(id to error) },
        )
    }

    @Test fun queuesAheadAndAcknowledgesOnlyConsumedFrames() {
        val device = Device()
        repeat(16) { assertTrue(device.queue.enqueue(byteArrayOf(1, 2, 3, 4), "$it")) }
        repeat(16) { device.queue.pump() }
        assertEquals(64, device.samples.size)
        assertTrue(device.completed.isEmpty())
        device.head = 16
        device.queue.pump()
        assertEquals((0..7).map { "$it" to null }, device.completed)
        assertTrue(device.queue.isNotEmpty)
        device.head = 32
        device.queue.pump()
        assertEquals(16, device.completed.size)
        assertFalse(device.queue.isNotEmpty)
    }

    @Test fun handlesPartialWritesAndBackpressureWithoutLosingOrRepeatingSamples() {
        val device = Device()
        device.queue.enqueue(byteArrayOf(1, 2, 3, 4, 5, 6), "packet")
        device.limit = 0
        repeat(4) { device.queue.pump() }
        assertTrue(device.samples.isEmpty())
        device.limit = 2
        repeat(3) { device.queue.pump() }
        assertEquals(listOf<Byte>(1, 2, 3, 4, 5, 6), device.samples)
        assertTrue(device.completed.isEmpty())
        device.head = 3
        device.queue.pump()
        assertEquals(listOf("packet" to null), device.completed)
    }

    @Test fun flushCancelsOldCompletionsAndResetsFrameAccounting() {
        val device = Device()
        device.queue.enqueue(ByteArray(8), "old")
        device.queue.pump()
        device.queue.clear()
        device.head = 0
        device.queue.enqueue(ByteArray(4), "new")
        device.queue.pump()
        assertTrue(device.completed.isEmpty())
        device.head = 2
        device.queue.pump()
        assertEquals(listOf("new" to null), device.completed)
    }

    @Test fun rejectsMalformedAndUnboundedAudioAndReleasesCapacityAfterConsumption() {
        val device = Device()
        assertFalse(device.queue.enqueue(ByteArray(0), "empty"))
        assertFalse(device.queue.enqueue(ByteArray(3), "odd"))
        assertFalse(device.queue.enqueue(ByteArray(512 * 1024 + 2), "large"))
        repeat(32) { assertTrue(device.queue.enqueue(ByteArray(2), "$it")) }
        assertFalse(device.queue.enqueue(ByteArray(2), "overflow"))
        repeat(32) { device.queue.pump() }
        device.head = 32
        device.queue.pump()
        assertTrue(device.queue.enqueue(ByteArray(2), "next"))
        device.queue.clear()
        repeat(2) { assertTrue(device.queue.enqueue(ByteArray(512 * 1024), "$it")) }
        assertFalse(device.queue.enqueue(ByteArray(2), "byte-overflow"))
    }

    @Test fun deviceFailureRejectsAllPendingAudio() {
        val device = Device()
        device.queue.enqueue(ByteArray(2), "a")
        device.queue.enqueue(ByteArray(2), "b")
        device.limit = -1
        device.queue.pump()
        assertEquals(listOf("a", "b"), device.completed.map { it.first })
        assertTrue(device.completed.all { it.second != null })
        assertFalse(device.queue.isNotEmpty)
    }
}
