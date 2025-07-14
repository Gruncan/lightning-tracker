import lzw
import binascii

# The compressed string from the user (converted from hex string to bytes)
compressed_hex = "800b6050220c0c8c340530b3303401b1200c8cfc304c8b91a5486d250119a4d2d2d23c67352d2dc548cdc9cccf4b0578492ccdc9c907007b04"
compressed_bytes = bytes.fromhex(compressed_hex)

# Try decompressing using lzw module
try:
    decompressed_data = lzw.read(io.BytesIO(compressed_bytes)).read()
    decompressed_data.decode('utf-8')
except Exception as e:
    decompressed_data = str(e)

print(decompressed_data)
