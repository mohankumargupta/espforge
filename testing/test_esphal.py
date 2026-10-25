from pytest_embedded import Dut

def test_hello_world(dut: Dut) -> None:
    # Wait for the "Hello World!" log from your ESP32-C3
    dut.expect(r"(?i)Hello World!")
    
# def test_typo(dut: Dut) -> None:
#     # Wait for the "Hello World!" log from your ESP32-C3
#     dut.expect(r"Hello Worl!")
