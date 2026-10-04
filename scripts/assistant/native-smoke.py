"""Run the real native protocol without recording the user's microphone."""
import json, os, queue, subprocess, sys, threading, time
binary = sys.argv[1]
process = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
responses = queue.Queue()
threading.Thread(target=lambda: [responses.put(json.loads(line)) for line in process.stdout], daemon=True).start()
def send(command, **fields):
    process.stdin.write(json.dumps(dict(command=command, **fields)) + '\n'); process.stdin.flush()
def receive(phase=None, timeout=40):
    until = time.monotonic() + timeout
    while True:
        response = responses.get(timeout=max(.01, until-time.monotonic()))
        if phase is None or response.get('phase') == phase: return response
try:
    send('status', id=0)
    status = receive('availability')
    assert status['imageInput'] is False
    print('Availability:', json.dumps(status))
    send('ask', id=1, text='')
    assert receive()['phase'] == 'error'
    send('stop', id=999)  # A release without a recording must not start work.
    send('cancel', id=2)
    assert receive()['phase'] == 'idle'
    print('Empty request and unmatched release/cancel: PASS')
    if status['model'] == 'available':
        send('ask', id=3, text='Write a single short friendly greeting. Text only; do not speak.')
        response = receive(timeout=60)
        assert response['phase'] == 'processing', response
        response = receive(timeout=60)
        assert response['phase'] == 'result' and response['text'] and response['action'] == 'preview', response
        print('Real on-device generation: PASS:', response['text'])
        send('ask', id=4, text='Write a short greeting and read it aloud.')
        receive('processing')
        response = receive(timeout=60)
        assert response['phase'] == 'result' and response['action'] == 'speak', response
        print('Real narration intent: PASS')
        send('ask', id=5, text='Write a long story about a forest.')
        receive('processing')
        send('cancel', id=6)
        assert receive()['phase'] == 'idle'
        send('status', id=0)
        receive('availability')
        time.sleep(2)
        while not responses.empty():
            assert responses.get()['phase'] != 'result', 'Canceled generation produced a result'
        print('Generation cancellation: PASS')
    else:
        print('SKIP real generation: model unavailable')
finally:
    process.stdin.close()
    try: process.wait(timeout=5)
    except subprocess.TimeoutExpired: process.kill(); process.wait()
