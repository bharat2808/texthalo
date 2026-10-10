import Foundation
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers

@main struct ReaderHost {
    static let allowedOrigin = "chrome-extension://jafnpokahhbldophkeogljemkblfmgmo/"
    struct Rect: Decodable { let x: Double; let y: Double; let width: Double; let height: Double }
    struct Viewport: Decodable { let width: Double; let height: Double }
    struct Request: Decodable {
        let protocolVersion: Int
        let requestId: String
        let type: String
        let url: String?
        let bookId: String?
        let region: Rect?
        let viewport: Viewport?
        let png: String?
        let payload: String?
    }
    static func failure(_ message: String) -> NSError { NSError(domain:"TextHalo.Reader",code:1,userInfo:[NSLocalizedDescriptionKey:message]) }
    static func readExact(_ count: Int) throws -> Data? {
        var data=Data()
        while data.count < count {
            guard let part=try FileHandle.standardInput.read(upToCount:count-data.count), !part.isEmpty else {
                if data.isEmpty {return nil};throw failure("Truncated native message")
            }
            data.append(part)
        }
        return data
    }
    static func write(_ value: [String:Any]) throws {
        let data=try JSONSerialization.data(withJSONObject:value)
        guard data.count < 1_000_000 else { throw failure("Response too large") }
        var length=UInt32(data.count).littleEndian
        try FileHandle.standardOutput.write(contentsOf:withUnsafeBytes(of:&length){Data($0)})
        try FileHandle.standardOutput.write(contentsOf:data)
    }
    static func process(_ request: Request) async throws -> [String:Any] {
        guard request.protocolVersion==1, UUID(uuidString:request.requestId) != nil else {throw failure("Invalid protocol or request ID")}
        if request.type=="hello" {return ["ok":true,"protocolVersion":1,"requestId":request.requestId,"capabilities":["local-ocr","app-relay"],"engine":"Apple Vision / TextHalo app relay"]}
        if request.type.hasPrefix("relay-") {return try await relay(request)}
        guard let string=request.url, let url=URL(string:string), url.scheme=="https",
              ["read.amazon.ca","read.amazon.com"].contains(url.host?.lowercased() ?? ""),
              let asin=URLComponents(url:url,resolvingAgainstBaseURL:false)?.queryItems?.first(where:{$0.name=="asin"})?.value,
              asin==request.bookId, asin.range(of:"^[A-Z0-9]{10}$",options:[.regularExpression,.caseInsensitive]) != nil else {throw failure("Invalid reader identity")}
        guard request.type=="ocr", let rect=request.region, let viewport=request.viewport, let encoded=request.png,
              encoded.hasPrefix("data:image/png;base64,"), encoded.count < 28_000_000 else {throw failure("Invalid OCR request")}
        let coordinates=[rect.x,rect.y,rect.width,rect.height,viewport.width,viewport.height]
        guard coordinates.allSatisfy({$0.isFinite}), viewport.width>0,viewport.height>0,rect.x>=0,rect.y>=0,rect.width>0,rect.height>0,
              rect.x+rect.width<=viewport.width+1, rect.y+rect.height<=viewport.height+1 else {throw failure("Crop exceeds viewport")}
        guard let data=Data(base64Encoded:String(encoded.dropFirst("data:image/png;base64,".count))),
              let source=CGImageSourceCreateWithData(data as CFData,nil),
              let props=CGImageSourceCopyPropertiesAtIndex(source,0,nil) as? [CFString:Any],
              let width=props[kCGImagePropertyPixelWidth] as? Int, let height=props[kCGImagePropertyPixelHeight] as? Int,
              width>0,height>0,width<=16000,height<=16000,width*height<=40_000_000,
              let image=CGImageSourceCreateImageAtIndex(source,0,nil) else {throw failure("Invalid or oversized PNG")}
        let scaleX=Double(width)/viewport.width,scaleY=Double(height)/viewport.height
        guard abs(scaleX-scaleY)<0.03 else {throw failure("Screenshot and viewport scale mismatch")}
        let bounds=CGRect(x:rect.x*scaleX,y:rect.y*scaleY,width:rect.width*scaleX,height:rect.height*scaleY).integral
        guard CGRect(x:0,y:0,width:width,height:height).contains(bounds),let crop=image.cropping(to:bounds) else {throw failure("Invalid image crop")}
        let lines=try await ScreenText.recognizeLines(crop)
        guard !lines.isEmpty else {throw failure("No readable text found in the page crop")}
        let text=ScreenText.reference(from:lines,limit:100_000)
        var response:[String:Any]=["ok":true,"protocolVersion":1,"requestId":request.requestId,"text":text,"lineCount":lines.count,"width":crop.width,"height":crop.height,"engine":"Apple Vision fast"]
        if let path=ProcessInfo.processInfo.environment["TEXTHALO_READER_OUTPUT"] {
            let dir=URL(fileURLWithPath:path,isDirectory:true).appendingPathComponent(request.requestId,isDirectory:true)
            try FileManager.default.createDirectory(at:dir,withIntermediateDirectories:true,attributes:[.posixPermissions:0o700])
            let png=dir.appendingPathComponent("book-area.png")
            guard let destination=CGImageDestinationCreateWithURL(png as CFURL,UTType.png.identifier as CFString,1,nil) else {throw failure("Cannot save crop")}
            CGImageDestinationAddImage(destination,crop,nil)
            guard CGImageDestinationFinalize(destination) else {throw failure("Cannot save crop")}
            try text.write(to:dir.appendingPathComponent("ocr.txt"),atomically:true,encoding:.utf8)
            response["outputDirectory"]=dir.path
        }
        return response
    }
    static func relay(_ request: Request) async throws -> [String:Any] {
        let routes=["relay-register":"register","relay-poll":"poll","relay-reply":"reply","relay-disconnect":"disconnect","relay-controls":"controls","relay-control":"control"]
        guard let route=routes[request.type],let payload=request.payload,payload.utf8.count<900_000,
              let body=payload.data(using:.utf8), (try JSONSerialization.jsonObject(with:body)) is [String:Any] else {throw failure("Invalid app relay request")}
        let config=FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent("Library/Application Support/kiegen/reader-bridge.json")
        guard let info=try? JSONSerialization.jsonObject(with:Data(contentsOf:config)) as? [String:Any],
              let port=info["port"] as? Int,port>0,port<=65535,
              let token=info["token"] as? String,UUID(uuidString:token) != nil else {throw failure("Open the updated TextHalo app to connect the reader relay.")}
        var call=URLRequest(url:URL(string:"http://127.0.0.1:\(port)/\(route)")!);call.httpMethod="POST"
        call.setValue("Bearer \(token)",forHTTPHeaderField:"Authorization");call.setValue("application/json",forHTTPHeaderField:"Content-Type")
        call.httpBody=body;call.timeoutInterval=8
        let (data,response)=try await URLSession.shared.data(for:call)
        guard data.count<950_000,
              let result=try JSONSerialization.jsonObject(with:data) as? [String:Any] else {throw failure("Invalid TextHalo app reply.")}
        guard (response as? HTTPURLResponse)?.statusCode==200 else {
            throw failure(result["error"] as? String ?? "The TextHalo app relay refused the request.")
        }
        return ["ok":true,"protocolVersion":1,"requestId":request.requestId,"data":result]
    }
    static func main() async {
        guard CommandLine.arguments.dropFirst().first==allowedOrigin else {
            if let path=ProcessInfo.processInfo.environment["TEXTHALO_READER_OUTPUT"] {
                let arguments=Array(CommandLine.arguments.dropFirst()).map { String($0.prefix(300)) }
                if let data=try? JSONSerialization.data(withJSONObject:["arguments":arguments]) {
                    try? data.write(to:URL(fileURLWithPath:path).appendingPathComponent("origin-rejected.json"))
                }
            }
            exit(2)
        }
        do {
            while let header=try readExact(4) {
                let length=header.withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)}.littleEndian
                guard length>0,length<=32_000_000 else {throw failure("Native message exceeds limit")}
                guard let data=try readExact(Int(length)) else {throw failure("Missing native message")}
                var requestID=""
                do {
                    let request=try JSONDecoder().decode(Request.self,from:data);requestID=request.requestId
                    try write(try await process(request))
                } catch {try write(["ok":false,"protocolVersion":1,"requestId":requestID,"error":error.localizedDescription])}
            }
        } catch {try? FileHandle.standardError.write(contentsOf:Data((error.localizedDescription+"\n").utf8));exit(1)}
    }
}
