import android.annotation.SuppressLint
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioDeviceCallback
import android.media.AudioDeviceInfo
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioRecord
import android.media.AudioTrack
import android.media.MediaRecorder
import android.media.audiofx.AcousticEchoCanceler
import android.media.audiofx.NoiseSuppressor
import android.os.Build
import android.os.PowerManager
import android.util.Log
import androidx.annotation.RequiresApi
import java.util.concurrent.Executors
import expo.modules.twowayaudio.pcm16VolumeLevel
import expo.modules.twowayaudio.PcmPlaybackQueue


class AudioEngine (context: Context) {
    private val SAMPLE_RATE = 16000
    private val AUDIO_FORMAT = AudioFormat.ENCODING_PCM_16BIT
    private val CHANNEL_CONFIG = AudioFormat.CHANNEL_IN_MONO

    private lateinit var audioRecord: AudioRecord
    private lateinit var audioManager: AudioManager
    private lateinit var audioTrack: AudioTrack
    private var audioFocusRequest: AudioFocusRequest? = null
    private var audioDeviceCallback: AudioDeviceCallback? = null
    private var playbackWorkerRunning = false
    private val playbackQueue = PcmPlaybackQueue(
        write = { data, offset, length ->
            audioTrack.write(data, offset, length, AudioTrack.WRITE_NON_BLOCKING).also { written ->
                if (written > 0) {
                    playbackWrites += 1
                    playbackWriteBytes += written
                    onOutputVolumeCallback?.invoke(pcm16VolumeLevel(data))
                    flushBridgeStats("write")
                }
            }
        },
        position = { audioTrack.playbackHeadPosition },
        complete = { id, error -> onPlaybackComplete?.invoke(id, error) },
    )
    private var echoCanceler: AcousticEchoCanceler? = null
    private var noiseSuppressor: NoiseSuppressor? = null
    private val executorServiceMicrophone = Executors.newFixedThreadPool(1)
    private val executorServicePlayback = Executors.newFixedThreadPool(1)
    private var speakerDevice: AudioDeviceInfo? = null
    private var communicationRouteActive = false
    private var bridgeWindowStartedAtMs = System.currentTimeMillis()
    private var micEvents = 0
    private var micBytes = 0L
    private var playbackEvents = 0
    private var playbackQueuedBytes = 0L
    private var playbackWrites = 0
    private var playbackWriteBytes = 0L
    @Volatile private var destroyed = false

    @Volatile var isRecording = false
    private var isRecordingBeforePause = false
    @Volatile var isPlaying = false

    // Callbacks
    var onPlaybackComplete: ((String, String?) -> Unit)? = null
    var onMicDataCallback: ((ByteArray) -> Unit)? = null
    var onInputVolumeCallback: ((Float) -> Unit)? = null
    var onOutputVolumeCallback: ((Float) -> Unit)? = null
    var onAudioInterruptionCallback: ((String) -> Unit)? = null

    init {
        initializeAudio(context)
    }

    private fun flushBridgeStats(reason: String) {
        val now = System.currentTimeMillis()
        val elapsedMs = now - bridgeWindowStartedAtMs
        if (elapsedMs < 1000) {
            return
        }
        Log.d(
            "AudioEngine",
            "[bridge] $reason mic=$micEvents" +
                "ev/${micBytes}B playbackQueue=$playbackEvents" +
                "ev/${playbackQueuedBytes}B playbackWrites=$playbackWrites" +
                "ev/${playbackWriteBytes}B windowMs=$elapsedMs"
        )
        bridgeWindowStartedAtMs = now
        micEvents = 0
        micBytes = 0
        playbackEvents = 0
        playbackQueuedBytes = 0
        playbackWrites = 0
        playbackWriteBytes = 0
    }

    @SuppressLint("NewApi")
    private fun initializeAudio(context:Context) {
        audioManager = context.getSystemService(Context.AUDIO_SERVICE) as AudioManager
        activateCommunicationRoute()
        if (!requestAudioFocus()) {
            handleAudioFocusBlocked()
        }

        // Listen for changes in audio routing
        val deviceCallback = object : AudioDeviceCallback() {
            override fun onAudioDevicesAdded(addedDevices: Array<out AudioDeviceInfo>?) {
                Log.d("AudioEngine", "onAudioDevicesAdded")
                super.onAudioDevicesAdded(addedDevices)
                if (communicationRouteActive) {
                    updateAudioRouting()
                }
            }
            override fun onAudioDevicesRemoved(removedDevices: Array<out AudioDeviceInfo>?) {
                Log.d("AudioEngine", "onAudioDevicesRemoved")
                super.onAudioDevicesRemoved(removedDevices)
                if (communicationRouteActive) {
                    updateAudioRouting()
                }
            }
        }
        audioManager.registerAudioDeviceCallback(deviceCallback, null)
        audioDeviceCallback = deviceCallback

        val bufferSize = AudioTrack.getMinBufferSize(
            SAMPLE_RATE,
            AudioFormat.CHANNEL_OUT_MONO,
            AUDIO_FORMAT
        )

        audioTrack = AudioTrack(
            AudioAttributes.Builder()
                .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                .build(),
            AudioFormat.Builder()
                .setEncoding(AUDIO_FORMAT)
                .setSampleRate(SAMPLE_RATE)
                .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                .build(),
            bufferSize,
            AudioTrack.MODE_STREAM,
            audioManager.generateAudioSessionId()
        ).apply {
            play()
        }
    }

    private fun updateAudioRouting() {
        val devices = audioManager.getDevices(AudioManager.GET_DEVICES_OUTPUTS)
        var isExternalDeviceConnected = false
        var selectedDevice: AudioDeviceInfo? = null

        for (device in devices) {
            if (device.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER) {
                speakerDevice = device
            }
            if (device.type == AudioDeviceInfo.TYPE_WIRED_HEADPHONES ||
                device.type == AudioDeviceInfo.TYPE_WIRED_HEADSET ||
                device.type == AudioDeviceInfo.TYPE_BLUETOOTH_A2DP ||
                device.type == AudioDeviceInfo.TYPE_BLUETOOTH_SCO) {
                isExternalDeviceConnected = true
                selectedDevice = device
                break
            } else if (device.type == AudioDeviceInfo.TYPE_BUILTIN_SPEAKER) {
                selectedDevice = device
            }
        }

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            // Use the modern API for Android S and above
            try {
                selectedDevice?.let {
                    audioManager.setCommunicationDevice(it)
                }
            }catch (e:Exception){
                Log.e("AudioEngine", "Error setting communication device. Using speaker")
                speakerDevice?.let {
                    audioManager.setCommunicationDevice(it)
                }
            }

        } else {
            // Fall back to deprecated method for older Android versions
            @Suppress("DEPRECATION")
            audioManager.isSpeakerphoneOn = !isExternalDeviceConnected
        }
    }

    @SuppressLint("NewApi")
    private fun activateCommunicationRoute() {
        communicationRouteActive = true
        audioManager.mode = AudioManager.MODE_IN_COMMUNICATION
        updateAudioRouting()
    }

    @SuppressLint("NewApi")
    private fun releaseCommunicationRoute() {
        communicationRouteActive = false
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            audioManager.clearCommunicationDevice()
        } else {
            @Suppress("DEPRECATION")
            audioManager.isSpeakerphoneOn = false
        }
        audioManager.mode = AudioManager.MODE_NORMAL
    }

    /**
     * Give the complete communication session back once we are neither recording nor playing.
     * Focus pauses the user's music, while MODE_IN_COMMUNICATION and the selected communication
     * device can leave Bluetooth earbuds on their narrow-band call route after capture stops.
     */
    @SuppressLint("NewApi")
    fun releaseAudioSession() {
        if (isRecording || isPlaying) {
            return
        }
        audioFocusRequest?.let { request ->
            audioManager.abandonAudioFocusRequest(request)
            audioFocusRequest = null
        }
        if (::audioTrack.isInitialized) {
            audioTrack.pause()
        }
        releaseCommunicationRoute()
    }

    /**
     * Take the audio session back before playing, mirroring iOS `activateAudioSessionIfNeeded()`.
     *
     * `releaseAudioSession()` abandons focus and pauses the track once we go idle, so a playback
     * turn that starts after that release has to reclaim both. Without this, the assistant's
     * response plays with no focus held and competes with whatever the user is listening to.
     * A null [audioFocusRequest] is what "we released" looks like, so it is the guard against
     * re-requesting focus mid-turn.
     *
     * Playback completion follows AudioTrack's frame counter. A delayed focus grant never
     * turns queued bytes into a premature played acknowledgement.
     */
    @SuppressLint("NewApi")
    private fun acquireAudioSessionIfNeeded() {
        if (audioFocusRequest != null) {
            return
        }
        activateCommunicationRoute()
        requestAudioFocus()
        if (::audioTrack.isInitialized) {
            audioTrack.play()
        }
    }

    @SuppressLint("NewApi")
    private fun requestAudioFocus(): Boolean {
        audioFocusRequest?.let { request ->
            audioManager.abandonAudioFocusRequest(request)
            audioFocusRequest = null
        }

        val focusRequest =
            AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE)
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                        .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH)
                        .build()
                )
                .setAcceptsDelayedFocusGain(true)
                .setOnAudioFocusChangeListener { focusChange ->
                    when (focusChange) {
                        AudioManager.AUDIOFOCUS_GAIN -> {
                            Log.d("AudioEngine", "Audio focus gained")
                        }
                        AudioManager.AUDIOFOCUS_LOSS -> {
                            Log.d("AudioEngine", "Audio focus lost")
                            handleAudioFocusBlocked()
                        }
                        AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> {
                            Log.d("AudioEngine", "Audio focus lost transiently")
                            handleAudioFocusBlocked()
                        }
                        AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> {
                            Log.d("AudioEngine", "Audio focus duck requested")
                        }
                    }
                }
                .build()

        audioFocusRequest = focusRequest
        val result = audioManager.requestAudioFocus(focusRequest)

        if (result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED) {
            return true
        }

        Log.w("AudioEngine", "Audio focus request was not granted: $result")
        return false
    }

    private fun handleAudioFocusBlocked() {
        if (isRecording) {
            stopRecording()
        }
        audioFocusRequest?.let { request ->
            audioManager.abandonAudioFocusRequest(request)
            audioFocusRequest = null
        }
        if (::audioTrack.isInitialized) {
            audioTrack.pause()
        }
        releaseCommunicationRoute()
        stopPlayback()
        isRecordingBeforePause = false
        onOutputVolumeCallback?.invoke(0.0F)
        onAudioInterruptionCallback?.let { it("blocked") }
    }

    @RequiresApi(Build.VERSION_CODES.Q)
    @SuppressLint("MissingPermission")
    private fun startRecording(): Boolean {
        activateCommunicationRoute()
        if (!requestAudioFocus()) {
            handleAudioFocusBlocked()
            return false
        }

        val bufferSize = AudioRecord.getMinBufferSize(SAMPLE_RATE, CHANNEL_CONFIG, AUDIO_FORMAT)
        audioRecord = AudioRecord(
            MediaRecorder.AudioSource.VOICE_COMMUNICATION,
            SAMPLE_RATE,
            CHANNEL_CONFIG,
            AUDIO_FORMAT,
            bufferSize
        )

        if (audioRecord.state != AudioRecord.STATE_INITIALIZED) {
            throw RuntimeException("Audio Record can't initialize!")
        }

        if (AcousticEchoCanceler.isAvailable()){
            echoCanceler = AcousticEchoCanceler.create(audioRecord.audioSessionId)
            if (echoCanceler != null) {
                echoCanceler?.enabled = true
                Log.i("AudioEngine", "Echo Canceler enabled")
            }
        }

        if (NoiseSuppressor.isAvailable()){
            noiseSuppressor = NoiseSuppressor.create(audioRecord.audioSessionId)
            if (noiseSuppressor != null) {
                noiseSuppressor?.enabled = true
                Log.i("AudioEngine", "Noise Suppressor enabled")
            }
        }

        audioRecord.startRecording()
        isRecording = true
        startMicSampleTap()
        return true
    }

    private fun startMicSampleTap(){
        executorServiceMicrophone.execute {
            val buffer = ByteArray(1024)
            try {
                while (isRecording) {
                    val read = audioRecord.read(buffer, 0, buffer.size)
                    if (read > 0) {
                        val data = buffer.copyOf(read)
                        micEvents += 1
                        micBytes += data.size.toLong()
                        flushBridgeStats("mic")
                        val micVolume = pcm16VolumeLevel(data)
                        onInputVolumeCallback?.invoke(micVolume)
                        onMicDataCallback?.invoke(data)
                    }
                }
                Log.d("AudioEngine", "Mic sample tap stopped.")
            }catch (e: Exception){
                Log.e("AudioEngine", "Error reading mic sample data", e)
                isRecording = false
                tearDown()
                throw e
            }
        }
    }

    private fun stopRecording() {
        if (!isRecording) return
        isRecording = false
        try {
            if (audioRecord.recordingState == AudioRecord.RECORDSTATE_RECORDING) {
                audioRecord.stop()
            }
        } finally {
            audioRecord.release()
            echoCanceler?.release()
            echoCanceler = null
            noiseSuppressor?.release()
            noiseSuppressor = null
        }
        onInputVolumeCallback?.invoke(0.0F)
    }

    @RequiresApi(Build.VERSION_CODES.Q)
    @Synchronized
    fun toggleRecording(value: Boolean): Boolean {
        if (destroyed) return false
        if (value == isRecording) return isRecording

        if (value) {
            return startRecording()
        } else {
            stopRecording()
        }

        isRecording = value
        return isRecording
    }

    @SuppressLint("NewApi")
    @Synchronized
    fun playPCMData(data: ByteArray, id: String? = null): Boolean {
        if (destroyed || !playbackQueue.enqueue(data, id)) return false
        acquireAudioSessionIfNeeded()
        playbackEvents += 1
        playbackQueuedBytes += data.size
        flushBridgeStats("queue")
        isPlaying = true
        if (!playbackWorkerRunning) {
            playbackWorkerRunning = true
            executorServicePlayback.execute { pumpPlayback() }
        }
        return true
    }

    private fun pumpPlayback() {
        try {
            while (true) {
                synchronized(this) {
                    if (destroyed) {
                        playbackWorkerRunning = false
                        return
                    }
                    if (!playbackQueue.pump()) stopPlayback()
                    isPlaying = playbackQueue.isNotEmpty
                    if (!isPlaying) {
                        playbackWorkerRunning = false
                        onOutputVolumeCallback?.invoke(0.0F)
                        return
                    }
                }
                // Poll the native device counter, including the tail after all bytes were
                // written. Audio continues independently of the JS event loop.
                Thread.sleep(5)
            }
        } catch (_: InterruptedException) {
            synchronized(this) { playbackWorkerRunning = false }
        } catch (error: Exception) {
            synchronized(this) {
                playbackWorkerRunning = false
                stopPlayback()
                onAudioInterruptionCallback?.invoke("blocked")
            }
            Log.e("AudioEngine", "Audio playback failed", error)
        }
    }

    fun bypassVoiceProcessing(bypass: Boolean) {
        if (bypass) {
            echoCanceler?.enabled = false
            noiseSuppressor?.enabled = false
        } else {
            echoCanceler?.enabled = true
            noiseSuppressor?.enabled = true
        }
    }

    @RequiresApi(Build.VERSION_CODES.Q)
    fun pauseRecordingAndPlayer() {
        isRecordingBeforePause = isRecording
        isRecording = toggleRecording(false)
        audioTrack.pause()
    }

    @RequiresApi(Build.VERSION_CODES.Q)
    fun resumeRecordingAndPlayer() {
        val wasRecordingBeforePause = isRecordingBeforePause
        // Only take the session back if something was actually live when we backgrounded. The
        // activity lifecycle listener calls this on every onResume, so requesting
        // AUDIOFOCUS_GAIN_TRANSIENT_EXCLUSIVE unconditionally re-paused the user's music every
        // time the app came to the foreground — the exact symptom this change exists to remove.
        if (!wasRecordingBeforePause && !isPlaying) {
            return
        }
        activateCommunicationRoute()
        if (!requestAudioFocus()) {
            handleAudioFocusBlocked()
            return
        }
        isRecording = toggleRecording(wasRecordingBeforePause)
        if (wasRecordingBeforePause && !isRecording) {
            return
        }
        audioTrack.play()
    }

    @Synchronized
    fun stopPlayback() {
        playbackQueue.clear()
        if (::audioTrack.isInitialized) {
            audioTrack.pause()
            audioTrack.flush()
        }
        isPlaying = false
        onOutputVolumeCallback?.invoke(0.0F)
        Log.d("AudioEngine", "Playback stopped")
    }

    fun pausePlayback() {
        audioTrack.pause()
        Log.d("AudioEngine", "Playback paused")
    }

    @SuppressLint("NewApi")
    fun resumePlayback() {
        acquireAudioSessionIfNeeded()
        audioTrack.play()
        Log.d("AudioEngine", "Playback resumed")
    }

    @SuppressLint("NewApi")
    @Synchronized
    fun tearDown() {
        if (destroyed) return
        destroyed = true
        try {
            stopRecording()
        } finally {
            audioDeviceCallback?.let { audioManager.unregisterAudioDeviceCallback(it) }
            audioDeviceCallback = null
            releaseCommunicationRoute()
            audioFocusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
            audioFocusRequest = null
            executorServiceMicrophone.shutdownNow()
            executorServicePlayback.shutdownNow()
            playbackQueue.clear()
            isPlaying = false
            echoCanceler?.release()
            echoCanceler = null
            noiseSuppressor?.release()
            noiseSuppressor = null
            if (::audioTrack.isInitialized) {
                try {
                    audioTrack.stop()
                } finally {
                    audioTrack.release()
                }
            }
        }
    }
}
